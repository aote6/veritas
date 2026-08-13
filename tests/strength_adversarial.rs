//! Veritas Strength / Adversarial Regression Suite
//!
//! Goal: pressure, attack, fault-injection, and boundary tests.
//! Principle: only tests. No production-code changes.
//! Every failure must be classified: true bug / wrong assumption /
//! unsupported / known debt / infra issue.

use std::sync::Arc;
use std::thread;
use std::time::Instant;

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{ObjectState, ObjectType, VeritasError};
use veritas_kernel::world_api::{WorldError, WorldService};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_strength_{}_{}_{}.wal",
        name,
        std::process::id(),
        fastrand::u64(..)
    ));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
}

fn world_with_wal(name: &str) -> (String, Arc<Kernel>, WorldService) {
    let wal = temp_wal(name);
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::with_wal(Arc::clone(&kernel), wal.clone());
    (wal, kernel, world)
}

fn assert_alive(world: &WorldService, id: u64, label: &str) {
    let obj = world
        .get_object(id)
        .unwrap_or_else(|| panic!("{label} (id={id}) must exist"));
    assert_eq!(obj.state, ObjectState::Alive, "{label} (id={id}) must be Alive");
}

fn assert_absent(world: &WorldService, id: u64, label: &str) {
    assert!(
        world.get_object(id).is_none(),
        "{label} (id={id}) must not exist"
    );
}

fn is_permission_denied(err: &WorldError) -> bool {
    matches!(
        err,
        WorldError::Kernel(VeritasError::PermissionDenied) | WorldError::Msg(_)
    ) || format!("{err:?}").contains("PermissionDenied")
        || format!("{err}").contains("PermissionDenied")
        || format!("{err:?}").contains("permission")
}

fn birth_via_kernel(kernel: &Kernel) -> u64 {
    let mut tx = kernel.test_begin();
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

// =============================================================================
// A. Malicious identity / capability attacks
// =============================================================================

/// S-A01: Illegal grantor — session actor is A, attempt to grant as B without capability.
/// Expected: PermissionDenied (or equivalent). No residual capability.
#[test]
fn s_a01_illegal_grantor() {
    let (wal, kernel, world) = world_with_wal("s_a01");

    // Setup: create A and B as independent objects in separate txs.
    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    let c = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // Session as A: try to grant as B on resource C without holding any cap on B.
    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_capability_grant(sid, b, a, "AdminCap".into(), c);
    assert!(
        result.is_err(),
        "illegal grantor B from actor A must be rejected; got Ok({:?})",
        result.ok()
    );
    let err = result.unwrap_err();
    assert!(
        is_permission_denied(&err),
        "expected permission-style error, got {:?}",
        err
    );

    // Abort must leave no residual capability records for this grant.
    let _ = world.tx_abort(sid);
    let caps = kernel.test_capability_records();
    let leaked = caps
        .iter()
        .any(|r| r.holder == a && r.resource == c && r.active);
    assert!(!leaked, "aborted illegal grant must leave no active cap for A on C");

    cleanup(&wal);
}

/// S-A02: Grantor/grantee swap attack — grantor tries to grant to self on foreign resource
/// without capability on that resource.
#[test]
fn s_a02_grantor_grantee_swap_foreign_resource() {
    let (wal, _kernel, world) = world_with_wal("s_a02");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // A tries to grant AdminCap on B to A (self) without holding anything on B.
    let sid = world.tx_begin(Some(a)).unwrap();
    // Switching into B requires Call(B) authorization — should fail.
    let result = world.tx_capability_grant(sid, b, a, "AdminCap".into(), b);
    assert!(
        result.is_err(),
        "grant as foreign grantor without Call(B) must fail"
    );
    let _ = world.tx_abort(sid);
    cleanup(&wal);
}

/// S-A03: Cross-object write without capability must fail; state must not change.
#[test]
fn s_a03_cross_object_write_without_cap() {
    let (wal, kernel, world) = world_with_wal("s_a03");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_write(sid1, 0, b"B-secret".to_vec(), Some(b)).unwrap();
    world.tx_commit(sid1).unwrap();

    let root_before = kernel.state_root();

    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_write(sid, 0, b"A-hijack".to_vec(), Some(b));
    assert!(
        result.is_err(),
        "A must not write B without capability; got Ok"
    );

    // State root and B's data must be unchanged after failed write + abort.
    let _ = world.tx_abort(sid);
    assert_eq!(
        kernel.state_root(),
        root_before,
        "failed cross-object write must not change state_root"
    );

    // Read B in a new authorized session — data must still be original.
    let sid_r = world.tx_begin(Some(b)).unwrap();
    // Switch to B is self (actor=b), so read should work for self.
    let data = world.tx_read(sid_r, 0);
    // If read returns data, it must not be hijacked content.
    if let Ok(bytes) = data {
        assert_ne!(
            bytes.as_slice(),
            b"A-hijack",
            "B data must not contain hijacked payload"
        );
    }
    let _ = world.tx_abort(sid_r);
    cleanup(&wal);
}

/// S-A04: Self-access exemption must NOT authorize write on unrelated object.
/// current_object == A, capability_context == A, target == B → must require capability.
#[test]
fn s_a04_self_access_exemption_boundary() {
    let (wal, _kernel, world) = world_with_wal("s_a04");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    // A is creator of B → A holds pending AdminCap on B in this tx.
    // Commit so we start clean with only committed caps.
    world.tx_commit(sid0).unwrap();

    // New session as A. After commit, A should hold AdminCap on B (birth grant).
    // To test pure self-access exemption boundary: create C independently.
    let sid1 = world.tx_begin(None).unwrap();
    let c = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // A writes C without ever being granted anything on C.
    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_write(sid, 0, b"nope".to_vec(), Some(c));
    assert!(
        result.is_err(),
        "self-access exemption must not allow A to write unrelated C"
    );
    let _ = world.tx_abort(sid);

    // Self write on A must still succeed.
    let sid2 = world.tx_begin(Some(a)).unwrap();
    world
        .tx_write(sid2, 0, b"self-ok".to_vec(), Some(a))
        .expect("self write must succeed via exemption");
    world.tx_commit(sid2).unwrap();
    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    assert_alive(&world, c, "C");
    cleanup(&wal);
}

/// S-A05: After abort, pending capability must not authorize subsequent session.
#[test]
fn s_a05_abort_invalidates_pending_capability() {
    let (wal, kernel, world) = world_with_wal("s_a05");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // Session: A grants B AdminCap on A, then abort.
    let sid = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid, a, b, "AdminCap".into(), a)
        .expect("grant from self must succeed");
    world.tx_abort(sid).unwrap();

    // New session as B: must not be able to use the aborted grant to write A.
    let sid2 = world.tx_begin(Some(b)).unwrap();
    let result = world.tx_write(sid2, 0, b"stolen".to_vec(), Some(a));
    assert!(
        result.is_err(),
        "aborted pending capability must not authorize later use"
    );
    let _ = world.tx_abort(sid2);

    // No active capability for B on A.
    let caps = kernel.test_capability_records();
    assert!(
        !caps.iter().any(|r| r.holder == b && r.resource == a && r.active),
        "no active cap B→A after abort"
    );
    cleanup(&wal);
}

/// S-A06: After commit+revoke path (if available via KernelCall), old capability must not work.
/// Uses KernelCall::CapabilityRevoke directly.
#[test]
fn s_a06_revoke_then_use_denied() {
    let wal = temp_wal("s_a06");
    let kernel = Kernel::with_wal_path(wal.clone());

    let _a = birth_via_kernel(&kernel);
    let b = birth_via_kernel(&kernel);
    let resource = birth_via_kernel(&kernel);

    // Grant B a capability on resource (grantor=B for self-grant pattern used in existing tests).
    let mut tx = kernel.test_begin();
    let cap_id = match kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantor: b,
                grantee: b,
                capability_type: "read".into(),
                resource,
            },
        )
        .unwrap()
    {
        TrapResult::CapabilityId(id) => id,
        _ => panic!("expected CapabilityId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    assert!(kernel.test_engine().holds_capability(cap_id, b));

    // Revoke.
    let mut tx2 = kernel.test_begin();
    kernel
        .handle(
            &mut tx2,
            KernelCall::CapabilityRevoke {
                capability_id: cap_id,
                holder: b,
                cascade_override: Some(true),
            },
        )
        .expect("revoke must succeed");
    kernel.handle(&mut tx2, KernelCall::Commit).unwrap();

    assert!(
        !kernel.test_engine().holds_capability(cap_id, b),
        "after revoke, holds must be false"
    );
    cleanup(&wal);
}

/// S-A07: Freeze without capability must fail; object remains Alive.
#[test]
fn s_a07_unauthorized_freeze() {
    let (wal, _kernel, world) = world_with_wal("s_a07");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_freeze_object(sid, b);
    assert!(result.is_err(), "unauthorized freeze of B by A must fail");
    let _ = world.tx_abort(sid);

    assert_alive(&world, b, "B after failed freeze");
    cleanup(&wal);
}

/// S-A08: Death without capability must fail; object remains Alive.
#[test]
fn s_a08_unauthorized_death() {
    let (wal, _kernel, world) = world_with_wal("s_a08");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_death_object(sid, b);
    assert!(result.is_err(), "unauthorized death of B by A must fail");
    let _ = world.tx_abort(sid);

    assert_alive(&world, b, "B after failed death");
    cleanup(&wal);
}

// =============================================================================
// B. Session isolation
// =============================================================================

/// S-B01: Session A must not observe session B's uncommitted writes.
#[test]
fn s_b01_session_cannot_see_uncommitted_writes() {
    let (wal, _kernel, world) = world_with_wal("s_b01");

    let sid0 = world.tx_begin(None).unwrap();
    let obj = world.tx_create_object(sid0).unwrap();
    world.tx_write(sid0, 0, b"committed".to_vec(), Some(obj)).unwrap();
    world.tx_commit(sid0).unwrap();

    // Session B writes new data but does not commit.
    let sid_b = world.tx_begin(Some(obj)).unwrap();
    world
        .tx_write(sid_b, 0, b"uncommitted".to_vec(), Some(obj))
        .unwrap();

    // Session A reads — must see committed value (or empty), never uncommitted.
    // Note: with sessions Mutex, concurrent ops serialize; isolation still checked.
    let sid_a = world.tx_begin(Some(obj)).unwrap();
    let data = world.tx_read(sid_a, 0).unwrap_or_default();
    assert_ne!(
        data.as_slice(),
        b"uncommitted",
        "session A must not see session B uncommitted write"
    );
    // Prefer committed content when readable.
    if !data.is_empty() {
        assert_eq!(data.as_slice(), b"committed");
    }

    let _ = world.tx_abort(sid_a);
    let _ = world.tx_abort(sid_b);
    cleanup(&wal);
}

/// S-B02: Session A must not use session B's pending capability.
#[test]
fn s_b02_session_cannot_use_other_pending_cap() {
    let (wal, _kernel, world) = world_with_wal("s_b02");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // Session X: A grants B on A, leave uncommitted.
    let sid_x = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_x, a, b, "AdminCap".into(), a)
        .unwrap();

    // Session Y as B: try to write A using the pending (uncommitted) grant.
    let sid_y = world.tx_begin(Some(b)).unwrap();
    let result = world.tx_write(sid_y, 0, b"via-pending".to_vec(), Some(a));
    assert!(
        result.is_err(),
        "other session must not use pending capability of another session"
    );

    let _ = world.tx_abort(sid_y);
    let _ = world.tx_abort(sid_x);
    cleanup(&wal);
}

/// S-B03: Abort clears all pending objects — they must not appear after abort.
#[test]
fn s_b03_abort_clears_pending_objects() {
    let (wal, _kernel, world) = world_with_wal("s_b03");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_abort(sid).unwrap();

    assert_absent(&world, a, "A after abort");
    assert_absent(&world, b, "B after abort");
    cleanup(&wal);
}

/// S-B04: Using ended session must return NoSession, not panic.
#[test]
fn s_b04_ended_session_rejected() {
    let (wal, _kernel, world) = world_with_wal("s_b04");

    let sid = world.tx_begin(None).unwrap();
    let _ = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();

    // Post-commit operations on same session id must fail cleanly.
    let r1 = world.tx_create_object(sid);
    assert!(matches!(r1, Err(WorldError::NoSession(_))), "create after commit: {:?}", r1);

    let r2 = world.tx_commit(sid);
    assert!(matches!(r2, Err(WorldError::NoSession(_))), "commit after commit: {:?}", r2);

    let r3 = world.tx_abort(sid);
    assert!(matches!(r3, Err(WorldError::NoSession(_))), "abort after commit: {:?}", r3);

    let r4 = world.tx_write(sid, 0, b"x".to_vec(), None);
    assert!(matches!(r4, Err(WorldError::NoSession(_))), "write after commit: {:?}", r4);

    cleanup(&wal);
}

/// S-B05: Nonexistent session id must return NoSession.
#[test]
fn s_b05_nonexistent_session() {
    let (wal, _kernel, world) = world_with_wal("s_b05");
    let bogus: u64 = 999_999_999;
    assert!(matches!(
        world.tx_commit(bogus),
        Err(WorldError::NoSession(_))
    ));
    assert!(matches!(
        world.tx_abort(bogus),
        Err(WorldError::NoSession(_))
    ));
    assert!(matches!(
        world.tx_create_object(bogus),
        Err(WorldError::NoSession(_))
    ));
    cleanup(&wal);
}

// =============================================================================
// C / D. WAL fault injection & recovery / replay
// =============================================================================

/// S-W01: Truncate final WAL record at various offsets — recovery must not panic.
#[test]
fn s_w01_truncated_final_record_no_panic() {
    let wal = temp_wal("s_w01");
    {
        let kernel = Kernel::with_wal_path(wal.clone());
        let _ = birth_via_kernel(&kernel);
        let _ = birth_via_kernel(&kernel);
    }

    let original = std::fs::read(&wal).expect("read wal");
    assert!(!original.is_empty(), "WAL should have content");

    for trunc in [1usize, 7, 15, 31, 63, 127, original.len().saturating_sub(1)] {
        if trunc >= original.len() {
            continue;
        }
        let mut bytes = original.clone();
        bytes.truncate(bytes.len() - trunc);
        std::fs::write(&wal, &bytes).unwrap();

        // Must not panic.
        let kernel = Kernel::with_wal_path(wal.clone());
        let _ = kernel.list_object_ids();
        let _ = kernel.state_root();
    }
    cleanup(&wal);
}

/// S-W02: Flip single bytes across the WAL — recovery must not panic.
#[test]
fn s_w02_single_byte_corruption_no_panic() {
    let wal = temp_wal("s_w02");
    {
        let kernel = Kernel::with_wal_path(wal.clone());
        let a = birth_via_kernel(&kernel);
        let mut tx = kernel.test_begin_in_object(a);
        kernel.test_write(&mut tx, 0, b"payload".to_vec()).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }

    let original = std::fs::read(&wal).unwrap();
    let positions: Vec<usize> = (0..original.len()).step_by((original.len() / 8).max(1)).collect();

    for &pos in &positions {
        let mut bytes = original.clone();
        bytes[pos] ^= 0xFF;
        std::fs::write(&wal, &bytes).unwrap();
        let kernel = Kernel::with_wal_path(wal.clone());
        let _ = kernel.list_object_ids();
        let _ = kernel.state_root();
    }
    cleanup(&wal);
}

/// S-W03: Empty WAL recovery.
#[test]
fn s_w03_empty_wal() {
    let wal = temp_wal("s_w03");
    std::fs::write(&wal, b"").unwrap();
    let kernel = Kernel::with_wal_path(wal.clone());
    assert!(kernel.list_object_ids().is_empty());
    cleanup(&wal);
}

/// S-R01: Recovery is idempotent — N recoveries yield same object set and state_root.
#[test]
fn s_r01_recovery_idempotent() {
    let wal = temp_wal("s_r01");
    let (id_a, id_b, root0);
    {
        let kernel = Kernel::with_wal_path(wal.clone());
        id_a = birth_via_kernel(&kernel);
        id_b = birth_via_kernel(&kernel);
        let mut tx = kernel.test_begin_in_object(id_a);
        kernel.test_write(&mut tx, 0, b"data-a".to_vec()).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
        root0 = kernel.state_root();
        let _ = (id_a, id_b, root0);
    }

    let mut prev_ids = None;
    let mut prev_root = None;
    for i in 0..5 {
        let kernel = Kernel::with_wal_path(wal.clone());
        let mut ids = kernel.list_object_ids();
        ids.sort();
        let root = kernel.state_root();
        if let Some(ref p) = prev_ids {
            assert_eq!(ids, *p, "recovery {i}: object ids must match");
        }
        if let Some(r) = prev_root {
            assert_eq!(root, r, "recovery {i}: state_root must match");
        }
        prev_ids = Some(ids);
        prev_root = Some(root);
    }
    cleanup(&wal);
}

/// S-R02: Duplicate recovery of multi-object + link + capability world stays consistent.
#[test]
fn s_r02_complex_world_recovery_stable() {
    let (wal, kernel, world) = world_with_wal("s_r02");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"A".to_vec(), Some(a)).unwrap();
    world.tx_write(sid, 0, b"B".to_vec(), Some(b)).unwrap();
    world.tx_link(sid, a, b, "OWNS").unwrap();
    world
        .tx_capability_grant(sid, a, b, "AdminCap".into(), a)
        .unwrap();
    world.tx_commit(sid).unwrap();

    let root = kernel.state_root();
    let ids: Vec<_> = {
        let mut v = kernel.list_object_ids();
        v.sort();
        v
    };
    let links = kernel.list_links().len();
    let caps = kernel.test_capability_records().len();

    drop(world);
    drop(kernel);

    for round in 0..3 {
        let k = Kernel::with_wal_path(wal.clone());
        let mut recovered_ids = k.list_object_ids();
        recovered_ids.sort();
        assert_eq!(recovered_ids, ids, "round {round}: ids");
        assert_eq!(k.state_root(), root, "round {round}: root");
        assert_eq!(k.list_links().len(), links, "round {round}: links");
        assert_eq!(
            k.test_capability_records().len(),
            caps,
            "round {round}: caps"
        );
    }
    cleanup(&wal);
}

/// S-W04: Append a duplicate of the last complete WAL line — recovery must not panic
/// and must not invent extra objects beyond what CRC/parser accepts.
#[test]
fn s_w04_duplicate_wal_line() {
    let wal = temp_wal("s_w04");
    let expected_count;
    {
        let kernel = Kernel::with_wal_path(wal.clone());
        let _ = birth_via_kernel(&kernel);
        let _ = birth_via_kernel(&kernel);
        expected_count = kernel.list_object_ids().len();
    }

    let content = std::fs::read_to_string(&wal).unwrap();
    let last_line = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or("")
        .to_string();
    if !last_line.is_empty() {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&wal)
            .unwrap();
        writeln!(f, "{}", last_line).unwrap();
    }

    let kernel = Kernel::with_wal_path(wal.clone());
    // Must not panic. Object count should not explode unbounded.
    let count = kernel.list_object_ids().len();
    assert!(
        count <= expected_count + 2,
        "duplicate line must not invent many objects: got {count}, expected ~{expected_count}"
    );
    cleanup(&wal);
}

// =============================================================================
// G / H. ObjectId & data boundaries
// =============================================================================

/// S-G01: Operations on nonexistent ObjectId must fail cleanly.
#[test]
fn s_g01_nonexistent_object_ops() {
    let (wal, _kernel, world) = world_with_wal("s_g01");
    let ghost: u64 = 0xDEAD_BEEF_CAFE;

    // begin with nonexistent actor
    let r = world.tx_begin(Some(ghost));
    assert!(
        matches!(r, Err(WorldError::ObjectNotFound(_))),
        "tx_begin(ghost) must be ObjectNotFound: {:?}",
        r
    );

    let sid = world.tx_begin(None).unwrap();
    let _ = world.tx_create_object(sid).unwrap();

    assert!(world.tx_freeze_object(sid, ghost).is_err());
    assert!(world.tx_death_object(sid, ghost).is_err());
    assert!(world
        .tx_capability_grant(sid, ghost, ghost, "AdminCap".into(), ghost)
        .is_err());

    let _ = world.tx_abort(sid);
    cleanup(&wal);
}

/// S-G02: ObjectId 0 is not a valid actor / target for privileged ops.
#[test]
fn s_g02_object_id_zero_boundary() {
    let (wal, _kernel, world) = world_with_wal("s_g02");

    // actor=None → actor 0 path (allowed for begin with no identity).
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    assert_ne!(a, 0, "birthed object id must not be 0");

    // Freeze object 0 should fail (no such object or not authorized).
    assert!(world.tx_freeze_object(sid, 0).is_err());
    assert!(world.tx_death_object(sid, 0).is_err());

    world.tx_commit(sid).unwrap();
    assert_alive(&world, a, "A");
    cleanup(&wal);
}

/// S-H01: Empty payload write and read-back.
#[test]
fn s_h01_empty_and_large_payload() {
    let (wal, _kernel, world) = world_with_wal("s_h01");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();

    world.tx_write(sid, 0, Vec::new(), Some(a)).expect("empty write");
    world
        .tx_write(sid, 1, b"x".to_vec(), Some(a))
        .expect("1-byte write");

    // Moderate payload (64 KiB) — must not panic.
    let big = vec![0xABu8; 64 * 1024];
    world
        .tx_write(sid, 2, big.clone(), Some(a))
        .expect("64KiB write");
    world.tx_commit(sid).unwrap();

    let sid2 = world.tx_begin(Some(a)).unwrap();
    let r0 = world.tx_read(sid2, 0).unwrap_or_default();
    let r1 = world.tx_read(sid2, 1).unwrap_or_default();
    let r2 = world.tx_read(sid2, 2).unwrap_or_default();
    assert!(r0.is_empty() || r0 == vec![], "empty payload roundtrip");
    assert_eq!(r1, b"x");
    assert_eq!(r2, big);
    world.tx_commit(sid2).unwrap();
    cleanup(&wal);
}

/// S-H02: Overwrite same state multiple times; last write wins after commit.
#[test]
fn s_h02_multiple_overwrite_same_state() {
    let (wal, _kernel, world) = world_with_wal("s_h02");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"v1".to_vec(), Some(a)).unwrap();
    world.tx_write(sid, 0, b"v2".to_vec(), Some(a)).unwrap();
    world.tx_write(sid, 0, b"v3".to_vec(), Some(a)).unwrap();
    world.tx_commit(sid).unwrap();

    let sid2 = world.tx_begin(Some(a)).unwrap();
    let data = world.tx_read(sid2, 0).unwrap();
    assert_eq!(data, b"v3", "last write must win");
    world.tx_commit(sid2).unwrap();
    cleanup(&wal);
}

// =============================================================================
// I. Transaction state machine abuse
// =============================================================================

/// S-T01: Double commit must fail on second call (NoSession).
#[test]
fn s_t01_double_commit() {
    let (wal, _kernel, world) = world_with_wal("s_t01");
    let sid = world.tx_begin(None).unwrap();
    let _ = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();
    assert!(matches!(world.tx_commit(sid), Err(WorldError::NoSession(_))));
    cleanup(&wal);
}

/// S-T02: Double abort.
#[test]
fn s_t02_double_abort() {
    let (wal, _kernel, world) = world_with_wal("s_t02");
    let sid = world.tx_begin(None).unwrap();
    let _ = world.tx_create_object(sid).unwrap();
    world.tx_abort(sid).unwrap();
    assert!(matches!(world.tx_abort(sid), Err(WorldError::NoSession(_))));
    cleanup(&wal);
}

/// S-T03: Commit then abort on same id.
#[test]
fn s_t03_commit_then_abort() {
    let (wal, _kernel, world) = world_with_wal("s_t03");
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();
    assert!(matches!(world.tx_abort(sid), Err(WorldError::NoSession(_))));
    assert_alive(&world, a, "A after commit");
    cleanup(&wal);
}

/// S-T04: Abort then commit on same id.
#[test]
fn s_t04_abort_then_commit() {
    let (wal, _kernel, world) = world_with_wal("s_t04");
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_abort(sid).unwrap();
    assert!(matches!(world.tx_commit(sid), Err(WorldError::NoSession(_))));
    assert_absent(&world, a, "A after abort");
    cleanup(&wal);
}

/// S-T05: begin → begin (two concurrent sessions) is allowed by architecture.
#[test]
fn s_t05_multiple_sessions_allowed() {
    let (wal, _kernel, world) = world_with_wal("s_t05");
    let s1 = world.tx_begin(None).unwrap();
    let s2 = world.tx_begin(None).unwrap();
    assert_ne!(s1, s2);
    let a = world.tx_create_object(s1).unwrap();
    let b = world.tx_create_object(s2).unwrap();
    world.tx_commit(s1).unwrap();
    world.tx_commit(s2).unwrap();
    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    cleanup(&wal);
}

// =============================================================================
// E. Stress (Level 1 / Level 2)
// =============================================================================

/// S-E01: Level-1 stress — 100 objects in separate transactions.
#[test]
fn s_e01_stress_100_objects() {
    let (wal, kernel, world) = world_with_wal("s_e01");
    let t0 = Instant::now();
    let mut ids = Vec::with_capacity(100);
    for _ in 0..100 {
        let sid = world.tx_begin(None).unwrap();
        let id = world.tx_create_object(sid).unwrap();
        world.tx_write(sid, 0, id.to_le_bytes().to_vec(), Some(id)).unwrap();
        world.tx_commit(sid).unwrap();
        ids.push(id);
    }
    let elapsed = t0.elapsed();
    assert_eq!(ids.len(), 100);
    for &id in &ids {
        assert_alive(&world, id, "stress object");
    }
    let root = kernel.state_root();
    // Recovery consistency
    drop(world);
    drop(kernel);
    let k2 = Kernel::with_wal_path(wal.clone());
    assert_eq!(k2.list_object_ids().len(), 100);
    assert_eq!(k2.state_root(), root);
    eprintln!("s_e01: 100 objects in {:?}, root={}", elapsed, root);
    cleanup(&wal);
}

/// S-E02: Level-1 multi-op single transaction — many writes.
#[test]
fn s_e02_stress_many_writes_one_tx() {
    let (wal, kernel, world) = world_with_wal("s_e02");
    let t0 = Instant::now();
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    for i in 0..200u64 {
        world
            .tx_write(sid, (i % 16) as u64, i.to_le_bytes().to_vec(), Some(a))
            .unwrap();
    }
    world.tx_commit(sid).unwrap();
    let elapsed = t0.elapsed();
    assert_alive(&world, a, "A");
    let _ = kernel.state_root();
    eprintln!("s_e02: 200 writes in {:?}", elapsed);
    cleanup(&wal);
}

/// S-E03: Level-2 stress — 1000 object births (separate txs). Skip under tight CI if needed.
#[test]
fn s_e03_stress_1000_objects() {
    let (wal, kernel, world) = world_with_wal("s_e03");
    let t0 = Instant::now();
    for i in 0..1000u64 {
        let sid = world.tx_begin(None).unwrap();
        let id = world.tx_create_object(sid).unwrap();
        if i % 10 == 0 {
            world
                .tx_write(sid, 0, i.to_le_bytes().to_vec(), Some(id))
                .unwrap();
        }
        world.tx_commit(sid).unwrap();
    }
    let elapsed = t0.elapsed();
    let count = kernel.list_object_ids().len();
    assert_eq!(count, 1000, "expected 1000 objects, got {count}");
    let root = kernel.state_root();
    drop(world);
    drop(kernel);
    let k2 = Kernel::with_wal_path(wal.clone());
    assert_eq!(k2.list_object_ids().len(), 1000);
    assert_eq!(k2.state_root(), root);
    eprintln!("s_e03: 1000 objects in {:?}, root={}", elapsed, root);
    cleanup(&wal);
}

/// S-E04: Wide capability graph — many grants from one holder.
#[test]
fn s_e04_stress_wide_capability_graph() {
    let (wal, kernel, world) = world_with_wal("s_e04");
    let sid = world.tx_begin(None).unwrap();
    let holder = world.tx_create_object(sid).unwrap();
    let mut resources = Vec::new();
    for _ in 0..50 {
        let r = world.tx_create_object(sid).unwrap();
        resources.push(r);
    }
    // holder is creator → has AdminCap on each resource (birth grant).
    // Additional explicit grants to a second object.
    let grantee = world.tx_create_object(sid).unwrap();
    for &r in &resources {
        // Grant from holder (self) to grantee on resource r.
        // Need to be in holder context — creator already is.
        world
            .tx_capability_grant(sid, holder, grantee, "AdminCap".into(), r)
            .expect("creator grant to grantee must succeed");
    }
    world.tx_commit(sid).unwrap();
    let caps = kernel.test_capability_records();
    assert!(!caps.is_empty(), "expected some capability records");
    assert_alive(&world, holder, "holder");
    assert_alive(&world, grantee, "grantee");
    cleanup(&wal);
}

// =============================================================================
// F. Concurrency (architecture-supported)
// =============================================================================

/// S-C01: Concurrent sessions writing different objects must both succeed.
/// WorldService serializes via sessions Mutex; Kernel uses per-structure Mutex.
#[test]
fn s_c01_concurrent_different_objects() {
    let (wal, kernel, world) = world_with_wal("s_c01");
    let world = Arc::new(world);

    // Pre-create two objects.
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();

    let w1 = Arc::clone(&world);
    let w2 = Arc::clone(&world);
    let h1 = thread::spawn(move || {
        let sid = w1.tx_begin(Some(a)).unwrap();
        for i in 0..20u64 {
            w1.tx_write(sid, 0, i.to_le_bytes().to_vec(), Some(a)).unwrap();
        }
        w1.tx_commit(sid).unwrap();
    });
    let h2 = thread::spawn(move || {
        let sid = w2.tx_begin(Some(b)).unwrap();
        for i in 0..20u64 {
            w2.tx_write(sid, 0, i.to_le_bytes().to_vec(), Some(b)).unwrap();
        }
        w2.tx_commit(sid).unwrap();
    });
    h1.join().expect("thread1 must not panic");
    h2.join().expect("thread2 must not panic");

    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    let _ = kernel.state_root();
    cleanup(&wal);
}

/// S-C02: Concurrent begin + commit of independent sessions.
#[test]
fn s_c02_concurrent_session_lifecycle() {
    let (wal, _kernel, world) = world_with_wal("s_c02");
    let world = Arc::new(world);
    let mut handles = Vec::new();
    for i in 0..8u64 {
        let w = Arc::clone(&world);
        handles.push(thread::spawn(move || {
            let sid = w.tx_begin(None).unwrap();
            let id = w.tx_create_object(sid).unwrap();
            w.tx_write(sid, 0, i.to_le_bytes().to_vec(), Some(id))
                .unwrap();
            w.tx_commit(sid).unwrap();
            id
        }));
    }
    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.join().expect("worker must not panic"));
    }
    assert_eq!(ids.len(), 8);
    for id in ids {
        assert_alive(&world, id, "concurrent object");
    }
    cleanup(&wal);
}

/// S-C03: Concurrent same-object writes — architecture may serialize or OCC-conflict.
/// Must not panic / deadlock. Final state must be one of the written values or clean error.
#[test]
fn s_c03_concurrent_same_object_writes() {
    let (wal, _kernel, world) = world_with_wal("s_c03");
    let world = Arc::new(world);

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"init".to_vec(), Some(a)).unwrap();
    world.tx_commit(sid).unwrap();

    let w1 = Arc::clone(&world);
    let w2 = Arc::clone(&world);
    let h1 = thread::spawn(move || {
        let sid = w1.tx_begin(Some(a)).unwrap();
        let r = w1.tx_write(sid, 0, b"t1".to_vec(), Some(a));
        let c = w1.tx_commit(sid);
        (r.is_ok(), c.is_ok())
    });
    let h2 = thread::spawn(move || {
        let sid = w2.tx_begin(Some(a)).unwrap();
        let r = w2.tx_write(sid, 0, b"t2".to_vec(), Some(a));
        let c = w2.tx_commit(sid);
        (r.is_ok(), c.is_ok())
    });
    let r1 = h1.join().expect("no panic");
    let r2 = h2.join().expect("no panic");
    // At least one path should complete without panic; both may succeed if serialized.
    assert!(
        r1.1 || r2.1 || r1.0 || r2.0,
        "at least some progress expected; got t1={r1:?} t2={r2:?}"
    );
    assert_alive(&world, a, "A still alive");
    cleanup(&wal);
}

// =============================================================================
// Additional capability graph attacks
// =============================================================================

/// S-A09: Link without capability on target must fail.
#[test]
fn s_a09_link_without_capability() {
    let (wal, _kernel, world) = world_with_wal("s_a09");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // A tries to link A→B without capability on B.
    // P4 confirmed: staging succeeds, commit rejects with PermissionDenied.
    let sid = world.tx_begin(Some(a)).unwrap();
    let result = world.tx_link(sid, a, b, "REFERENCES");
    assert!(result.is_ok(), "staging link must succeed (commit enforces auth)");
    let commit_result = world.tx_commit(sid);
    assert!(
        commit_result.is_err(),
        "commit must reject link without target capability"
    );
    cleanup(&wal);
}

/// S-A10: After successful grant+commit, grantee can use; grantor identity is not swapped.
#[test]
fn s_a10_grantor_does_not_become_grantee() {
    let (wal, kernel, world) = world_with_wal("s_a10");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    // A (creator/current) grants B AdminCap on A.
    world
        .tx_capability_grant(sid, a, b, "AdminCap".into(), a)
        .expect("grant A→B on A");
    world.tx_commit(sid).unwrap();

    let records = kernel.test_capability_records();
    let b_holds = records
        .iter()
        .any(|r| r.holder == b && r.resource == a && r.active);
    assert!(b_holds, "B must hold active capability on A");

    // B can write A in new session.
    let sid2 = world.tx_begin(Some(b)).unwrap();
    world
        .tx_write(sid2, 0, b"from-b".to_vec(), Some(a))
        .expect("B should write A with granted cap");
    world.tx_commit(sid2).unwrap();
    cleanup(&wal);
}
