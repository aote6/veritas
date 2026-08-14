//! P4: Veritas Security & Recovery Differential Audit
//!
//! Focused tests for three open questions from the strength suite:
//! 1. WorldService::tx_link vs Machine/Kernel ObjectLink authorization parity
//! 2. global_version semantics after WAL recovery (log vs real state)
//! 3. Structural WAL attacks (valid UTF-8 / CRC, broken semantics)
//!
//! Production code: 0 modifications.
//! Principle: prove facts; classify findings; do not fix production here.

use std::sync::Arc;

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{
    AccessIntent, Address, LinkType, ObjectType, PendingCapabilityDelegate,
    PendingCapabilityGrant, PendingCapabilityRevoke, ScopeChangeType, TransactionDelta,
    VeritasError, ZERO_HASH,
};
use veritas_kernel::wal::WalEntry;
use veritas_kernel::world_api::WorldService;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_audit_{}_{}_{}.wal",
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

fn world_pair(name: &str) -> (String, Arc<Kernel>, WorldService) {
    let wal = temp_wal(name);
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::with_wal(Arc::clone(&kernel), wal.clone());
    (wal, kernel, world)
}

fn birth_kernel(kernel: &Kernel) -> u64 {
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

fn birth_under(kernel: &Kernel, creator: u64) -> u64 {
    let mut tx = kernel.test_begin_in_object(creator);
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

fn snapshot_world(kernel: &Kernel) -> WorldSnap {
    let mut ids = kernel.list_object_ids();
    ids.sort();
    let mut links: Vec<(u64, u64, u8)> = kernel
        .list_links()
        .into_iter()
        .map(|l| (l.from, l.to, l.link_type as u8))
        .collect();
    links.sort();
    let mut caps: Vec<(u64, u64, u64, bool)> = kernel
        .test_capability_records()
        .into_iter()
        .map(|r| (r.holder, r.resource, r.capability_id, r.active))
        .collect();
    caps.sort();
    WorldSnap {
        ids,
        links,
        caps,
        root: kernel.state_root(),
        version: kernel.get_global_version(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorldSnap {
    ids: Vec<u64>,
    links: Vec<(u64, u64, u8)>,
    caps: Vec<(u64, u64, u64, bool)>,
    root: u64,
    version: u64,
}

fn append_wal_entry(path: &str, entry: &WalEntry) {
    use std::io::Write;
    let line = entry.serialize();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open wal for append");
    writeln!(f, "{}", line).expect("write wal line");
    f.flush().ok();
}

fn empty_delta(tx_id: u64, version: u64) -> TransactionDelta {
    TransactionDelta {
        tx_id,
        commit_version: version,
        actor_id: 0,
        writes: vec![],
        scope_changes: vec![],
        births: vec![],
        deaths: vec![],
        freezes: vec![],
        links: vec![],
        unlinks: vec![],
        capability_grants: vec![],
        capability_delegates: vec![],
        capability_revokes: vec![],
        effects: vec![],
    }
}

// =============================================================================
// Part 1: WorldService ↔ Machine / Kernel link authorization parity
// =============================================================================

/// Constitution-aligned expectation (from machine_object_link_security +
/// AccessIntent::Link(from,to) → target_objects = [from, to]):
///
/// Staging ObjectLink may succeed; **commit** must reject Link(A,B) when the
/// acting identity holds no capability on B (and B ≠ current_object /
/// capability_context).
///
/// WorldService::tx_link does not pre-authorize; it relies on the same
/// engine.commit → verify_capability path as Machine/Kernel.
#[test]
fn audit_link_worldservice_commit_rejects_without_target_cap() {
    let (wal, kernel, world) = world_pair("link_ws");

    // A and B born in separate committed transactions — no shared AdminCap.
    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    // Prove: A does not hold active capability on B.
    let caps = kernel.test_capability_records();
    assert!(
        !caps
            .iter()
            .any(|r| r.holder == a && r.resource == b && r.active),
        "precondition: A must not hold active cap on B"
    );

    let sid = world.tx_begin(Some(a)).unwrap();
    // Staging may succeed (ObjectLink only pushes pending_links).
    let stage = world.tx_link(sid, a, b, "REFERENCES");
    assert!(
        stage.is_ok(),
        "staging ObjectLink is allowed; auth is at commit. got {:?}",
        stage
    );

    // Commit must enforce AccessIntent::Link(A,B) → need cap on B.
    let commit = world.tx_commit(sid);
    assert!(
        commit.is_err(),
        "WorldService commit MUST reject Link(A,B) without capability on B; got Ok({:?})",
        commit.as_ref().ok()
    );
    // Prefer PermissionDenied classification when available.
    let err = commit.unwrap_err();
    let s = format!("{:?} {}", err, err);
    assert!(
        s.contains("PermissionDenied") || s.contains("permission") || s.contains("Kernel"),
        "expected permission-related error, got {:?}",
        err
    );

    assert!(
        !kernel.has_link(a, b),
        "no link edge after rejected WorldService commit"
    );
    cleanup(&wal);
}

/// Same topology via KernelCall path (Machine-equivalent authorization surface).
#[test]
fn audit_link_kernel_commit_rejects_without_target_cap() {
    let wal = temp_wal("link_k");
    let kernel = Kernel::with_wal_path(wal.clone());

    let a = birth_kernel(&kernel);
    // Independent B under a different creator so A has no AdminCap on B.
    let stranger = birth_kernel(&kernel);
    let b = birth_under(&kernel, stranger);

    let mut tx = kernel.test_begin_in_object(a);
    kernel
        .handle(
            &mut tx,
            KernelCall::ObjectLink {
                from: a,
                to: b,
                link_type: LinkType::References,
            },
        )
        .expect("staging ObjectLink ok");

    let commit = kernel.handle(&mut tx, KernelCall::Commit);
    assert!(
        commit.is_err(),
        "Kernel commit must reject Link without target cap; got Ok"
    );
    assert!(!kernel.has_link(a, b));
    cleanup(&wal);
}

/// With capability on target, both WorldService and Kernel commit succeed.
#[test]
fn audit_link_worldservice_succeeds_with_target_cap() {
    let (wal, kernel, world) = world_pair("link_ok");

    // Same creator session → A holds AdminCap on B after birth in-tx.
    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    // Creator context still A (or first object); pending AdminCap on B authorizes Link.
    world
        .tx_link(sid, a, b, "OWNS")
        .expect("stage link with creator pending AdminCap");
    world.tx_commit(sid).expect("commit with target cap must succeed");

    assert!(kernel.has_link(a, b), "link must exist after authorized commit");
    cleanup(&wal);
}

/// Direct authorize_intent on AccessIntent::Link(A,B) with actor A, no caps.
#[test]
fn audit_link_authorize_intent_requires_both_endpoints() {
    let wal = temp_wal("link_ai");
    let kernel = Kernel::with_wal_path(wal.clone());
    let a = birth_kernel(&kernel);
    let stranger = birth_kernel(&kernel);
    let b = birth_under(&kernel, stranger);

    let tx = kernel.test_begin_in_object(a);
    let intent = AccessIntent::Link(a, b);
    let r = kernel.test_authorize_intent(&tx, &intent);
    assert!(
        matches!(r, Err(VeritasError::PermissionDenied)),
        "authorize_intent(Link(A,B)) without cap on B must be PermissionDenied, got {:?}",
        r
    );

    // Self-link intent Link(A,A): both targets exempt via current_object.
    let self_intent = AccessIntent::Link(a, a);
    assert!(
        kernel.test_authorize_intent(&tx, &self_intent).is_ok(),
        "Link(A,A) is self-access exempt"
    );
    cleanup(&wal);
}

/// Parity table assertion: WorldService and Kernel yield same commit outcome
/// on identical object graph (no target cap).
#[test]
fn audit_link_worldservice_machine_parity() {
    // WorldService path
    let (wal_w, k_w, world) = world_pair("parity_w");
    let sid0 = world.tx_begin(None).unwrap();
    let a_w = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();
    let sid1 = world.tx_begin(None).unwrap();
    let b_w = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();
    let sid = world.tx_begin(Some(a_w)).unwrap();
    world.tx_link(sid, a_w, b_w, "REFERENCES").unwrap();
    let ws_commit_err = world.tx_commit(sid).is_err();
    let ws_has_link = k_w.has_link(a_w, b_w);

    // Kernel path
    let wal_k = temp_wal("parity_k");
    let k = Kernel::with_wal_path(wal_k.clone());
    let a = birth_kernel(&k);
    let stranger = birth_kernel(&k);
    let b = birth_under(&k, stranger);
    let mut tx = k.test_begin_in_object(a);
    k.handle(
        &mut tx,
        KernelCall::ObjectLink {
            from: a,
            to: b,
            link_type: LinkType::References,
        },
    )
    .unwrap();
    let k_commit_err = k.handle(&mut tx, KernelCall::Commit).is_err();
    let k_has_link = k.has_link(a, b);

    assert_eq!(
        ws_commit_err, k_commit_err,
        "WorldService vs Kernel commit rejection parity: ws_err={ws_commit_err} k_err={k_commit_err}"
    );
    assert_eq!(
        ws_has_link, k_has_link,
        "WorldService vs Kernel link presence parity"
    );
    assert!(
        ws_commit_err && k_commit_err,
        "both paths must reject unauthorized link at commit"
    );
    assert!(!ws_has_link && !k_has_link);

    cleanup(&wal_w);
    cleanup(&wal_k);
}

/// Contrast: WorldService pre-authorizes freeze/death/write switches, but NOT link.
/// This documents API design, not necessarily a bug — commit still gates link.
#[test]
fn audit_link_no_preauth_but_commit_gates() {
    let (wal, kernel, world) = world_pair("preauth");

    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();
    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_commit(sid1).unwrap();

    let sid = world.tx_begin(Some(a)).unwrap();
    // freeze of foreign B is rejected at the WorldService call (pre-auth).
    assert!(
        world.tx_freeze_object(sid, b).is_err(),
        "freeze pre-authorizes Call(B)"
    );
    // link stages without pre-auth.
    assert!(
        world.tx_link(sid, a, b, "REFERENCES").is_ok(),
        "link does not pre-authorize"
    );
    // but commit still rejects.
    assert!(world.tx_commit(sid).is_err());
    assert!(!kernel.has_link(a, b));
    cleanup(&wal);
}

// =============================================================================
// Part 2: Recovery global_version — real semantics, not log text
// =============================================================================

/// After N commits, recovery must restore get_global_version() to the last
/// commit_version (via apply()), even if recover()'s max_version log is 0.
#[test]
fn audit_recovery_restores_global_version() {
    let wal = temp_wal("ver_restore");
    let versions_before: Vec<u64>;
    let final_version: u64;
    let final_root: u64;
    let object_count: usize;

    {
        let kernel = Kernel::with_wal_path(wal.clone());
        let mut vs = Vec::new();
        for _ in 0..5 {
            let _ = birth_kernel(&kernel);
            vs.push(kernel.get_global_version());
        }
        versions_before = vs;
        final_version = kernel.get_global_version();
        final_root = kernel.state_root();
        object_count = kernel.list_object_ids().len();
        assert!(final_version >= 5, "expected version >= 5, got {final_version}");
        assert_eq!(
            versions_before.last().copied().unwrap(),
            final_version,
            "last recorded version must match final"
        );
    }

    // Recovery
    let kernel2 = Kernel::with_wal_path(wal.clone());
    let recovered_version = kernel2.get_global_version();
    let recovered_root = kernel2.state_root();
    let recovered_count = kernel2.list_object_ids().len();

    assert_eq!(
        recovered_count, object_count,
        "object count must survive recovery"
    );
    assert_eq!(
        recovered_root, final_root,
        "state_root must survive recovery"
    );
    assert_eq!(
        recovered_version, final_version,
        "get_global_version AFTER recovery must equal last commit_version \
         (if this fails: either apply() did not run, deltas have commit_version=0, \
         or recover-init overwrote apply). log '当前版本号' may still print 0 \
         because recover() max_version ignores TransactionCommitted."
    );
    cleanup(&wal);
}

/// After recovery, subsequent commits must continue version sequence.
#[test]
fn audit_recovery_version_continues_after_restart() {
    let wal = temp_wal("ver_cont");
    let v0: u64;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
        let _ = birth_kernel(&k);
        v0 = k.get_global_version();
    }

    let k2 = Kernel::with_wal_path(wal.clone());
    assert_eq!(k2.get_global_version(), v0, "version restored");
    let _ = birth_kernel(&k2);
    let v1 = k2.get_global_version();
    assert!(
        v1 > v0,
        "post-recovery commit must increase version: before={v0} after={v1}"
    );
    assert_eq!(v1, v0 + 1, "version must increment by 1 per commit");

    // Second recovery still consistent
    drop(k2);
    let k3 = Kernel::with_wal_path(wal.clone());
    assert_eq!(k3.get_global_version(), v1);
    cleanup(&wal);
}

/// receipts_since after recovery should see committed versions > since.
#[test]
fn audit_recovery_receipts_since_sees_history() {
    let (wal, _k, world) = world_pair("receipts");

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"x".to_vec(), Some(a)).unwrap();
    let receipt = world.tx_commit(sid).unwrap();
    let v = receipt.version;
    assert!(v > 0, "committed receipt version must be > 0, got {v}");

    // receipts_since(0) should include this commit when wal_path is set.
    let recs = world.receipts_since(0, None);
    assert!(
        !recs.is_empty(),
        "receipts_since(0) must return at least one receipt after commit"
    );
    assert!(
        recs.iter().any(|r| r.version == v),
        "receipts must include version {v}"
    );

    // New WorldService on same WAL (simulates restart).
    drop(world);
    let k2 = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world2 = WorldService::with_wal(Arc::clone(&k2), wal.clone());
    assert_eq!(k2.get_global_version(), v, "version after recovery");

    let recs2 = world2.receipts_since(0, None);
    assert!(
        !recs2.is_empty(),
        "receipts_since after recovery must still see history"
    );
    assert!(
        recs2.iter().any(|r| r.version == v),
        "recovered receipts must include version {v}"
    );

    // since == v → empty (strictly greater filter)
    let after = world2.receipts_since(v, None);
    assert!(
        after.iter().all(|r| r.version > v),
        "receipts_since(v) must only include versions > v"
    );
    cleanup(&wal);
}

/// OCC-related: snapshot_version is taken at begin; after recovery a new
/// transaction must see restored global_version as its baseline.
#[test]
fn audit_recovery_occ_baseline_matches_version() {
    let wal = temp_wal("occ");
    let v_final: u64;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let a = birth_kernel(&k);
        let mut tx = k.test_begin_in_object(a);
        k.test_write(&mut tx, 0, b"v1".to_vec()).unwrap();
        k.handle(&mut tx, KernelCall::Commit).unwrap();
        v_final = k.get_global_version();
    }

    let k2 = Kernel::with_wal_path(wal.clone());
    assert_eq!(k2.get_global_version(), v_final);

    // New tx begins at restored version as snapshot baseline.
    let a = k2.list_object_ids()[0];
    let tx = k2.test_begin_in_object(a);
    assert_eq!(
        tx.snapshot_version(),
        v_final,
        "new transaction snapshot_version must equal restored global_version"
    );
    cleanup(&wal);
}

/// Prove recover() max_version ignores TransactionCommitted while apply() fixes
/// engine.global_version — documents the log discrepancy root cause.
#[test]
fn audit_recover_max_version_ignores_txcommitted_but_apply_sets_engine() {
    let wal = temp_wal("maxver");
    let final_v: u64;
    {
        let k = Kernel::with_wal_path(wal.clone());
        for _ in 0..3 {
            let _ = birth_kernel(&k);
        }
        final_v = k.get_global_version();
        assert!(final_v >= 3);
    }

    // Direct RecoveryManager::recover — max_version often 0 for pure TXCOMMIT WALs.
    let (records, max_from_recover) =
        veritas_kernel::wal::RecoveryManager::recover(&wal).expect("recover ok");
    assert!(!records.is_empty());

    // All modern commits should be TransactionCommitted.
    let txc = records
        .iter()
        .filter(|e| matches!(e, WalEntry::TransactionCommitted(_)))
        .count();
    assert!(txc > 0, "expected TransactionCommitted records");

    // max_from_recover may be 0 if only TransactionCommitted is present.
    // That is the log bug source. Engine after with_wal_path must still be correct.
    let k2 = Kernel::with_wal_path(wal.clone());
    assert_eq!(
        k2.get_global_version(),
        final_v,
        "engine version correct via apply despite recover max_version={max_from_recover}"
    );

    if max_from_recover == 0 && final_v > 0 {
        eprintln!(
            "CONFIRMED: RecoveryManager::recover max_version={} while engine version={} \
             — log '当前版本号' prints recover() return, not post-apply global_version",
            max_from_recover, final_v
        );
    }
    cleanup(&wal);
}

// =============================================================================
// Part 3: Structural WAL attacks
// =============================================================================

/// Duplicate identical TransactionCommitted line appended — recovery must not
/// invent extra objects / explode state. Prefer idempotent or last-wins.
#[test]
fn audit_wal_duplicate_transaction_committed() {
    let wal = temp_wal("dup_txc");
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
        let _ = birth_kernel(&k);
        snap = snapshot_world(&k);
    }

    // Duplicate last line
    let content = std::fs::read_to_string(&wal).unwrap();
    let last = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap()
        .to_string();
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&wal)
            .unwrap();
        writeln!(f, "{}", last).unwrap();
        writeln!(f, "{}", last).unwrap();
    }

    let k2 = Kernel::with_wal_path(wal.clone());
    let snap2 = snapshot_world(&k2);

    // Must not panic. Object count must not grow unbounded.
    assert!(
        snap2.ids.len() <= snap.ids.len() + 2,
        "duplicate TXCOMMIT must not invent many objects: before={:?} after={:?}",
        snap.ids.len(),
        snap2.ids.len()
    );
    // Prefer exact idempotence if architecture guarantees it.
    if snap2 != snap {
        eprintln!(
            "OBSERVED: duplicate TransactionCommitted changed world.\n  before={:?}\n  after={:?}",
            snap, snap2
        );
        // Still: version should not go backwards relative to a sane baseline.
        // And we must not get empty world when original had objects.
        assert!(
            !snap2.ids.is_empty() || snap.ids.is_empty(),
            "must not wipe objects solely due to duplicate lines"
        );
    }
    cleanup(&wal);
}

/// Append a TransactionCommitted with a new birth id already used — recovery
/// behavior: may recreate / overwrite registry entry; must not panic.
#[test]
fn audit_wal_duplicate_birth_id_in_new_tx() {
    let wal = temp_wal("dup_birth");
    let existing_id: u64;
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        existing_id = birth_kernel(&k);
        snap = snapshot_world(&k);
    }

    // Craft a second tx that re-births the same object id with higher version.
    let mut delta = empty_delta(99, snap.version + 1);
    delta.births.push(existing_id);
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    let snap2 = snapshot_world(&k2);
    // No panic; object still present.
    assert!(
        snap2.ids.contains(&existing_id),
        "object id must still exist after duplicate birth record"
    );
    eprintln!(
        "dup birth: before_ver={} after_ver={} ids={:?}",
        snap.version, snap2.version, snap2.ids
    );
    cleanup(&wal);
}

/// TransactionCommitted with version going backwards relative to prior records.
#[test]
fn audit_wal_out_of_order_version() {
    let wal = temp_wal("ver_order");
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
        let _ = birth_kernel(&k);
        snap = snapshot_world(&k);
        assert!(snap.version >= 2);
    }

    // Append a delta with version=1 (lower than current).
    let mut delta = empty_delta(50, 1);
    delta.births.push(99999);
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    let snap2 = snapshot_world(&k2);
    // apply() sets global_version to last applied delta's commit_version.
    // Last delta has version 1 → engine version may become 1 (regression risk).
    eprintln!(
        "out-of-order version: before={} after={} ids_before={} ids_after={:?}",
        snap.version,
        snap2.version,
        snap.ids.len(),
        snap2.ids
    );
    assert!(
        snap2.version >= snap.version,
        "BUG C: global_version regressed from {} to {} after lower-version TXCOMMIT append",
        snap.version,
        snap2.version
    );
    let _ = k2.state_root();
    cleanup(&wal);
}

/// Duplicate link edges via appended TransactionCommitted.
#[test]
fn audit_wal_duplicate_link_records() {
    let wal = temp_wal("dup_link");
    let (a, b, snap): (u64, u64, WorldSnap);
    {
        let k = Kernel::with_wal_path(wal.clone());
        // Creator A births B under A so pending/committed AdminCap authorizes Link(A,B).
        let a_id = birth_kernel(&k);
        let b_id = birth_under(&k, a_id);
        let mut tx = k.test_begin_in_object(a_id);
        k.handle(
            &mut tx,
            KernelCall::ObjectLink {
                from: a_id,
                to: b_id,
                link_type: LinkType::References,
            },
        )
        .expect("stage link with creator AdminCap on B");
        k.handle(&mut tx, KernelCall::Commit)
            .expect("commit authorized link");
        a = a_id;
        b = b_id;
        snap = snapshot_world(&k);
        assert!(k.has_link(a, b));
    }

    // Append another TXCOMMIT that only re-adds the same link.
    let mut delta = empty_delta(80, snap.version + 1);
    delta.links.push((a, b, LinkType::References));
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    let links: Vec<_> = k2.list_links();
    let count_ab = links.iter().filter(|l| l.from == a && l.to == b).count();
    eprintln!("duplicate link recovery: edge count A→B = {count_ab}");
    assert_eq!(
        count_ab, 1,
        "BUG D: duplicate link recovery produced {count_ab} edges A->B, expected exactly 1"
    );
    assert!(k2.has_link(a, b));
    cleanup(&wal);
}

/// Capability grant duplicated via WAL append.
#[test]
fn audit_wal_duplicate_capability_grant() {
    let wal = temp_wal("dup_cap");
    let snap: WorldSnap;
    let (holder, resource, cap_id, seq): (u64, u64, u64, u64);
    {
        let k = Kernel::with_wal_path(wal.clone());
        let h = birth_kernel(&k);
        let r = birth_kernel(&k);
        let mut tx = k.test_begin_in_object(h);
        let cid = match k
            .handle(
                &mut tx,
                KernelCall::CapabilityGrant {
                    grantor: h,
                    grantee: h,
                    capability_type: "read".into(),
                    resource: r,
                },
            )
            .unwrap()
        {
            TrapResult::CapabilityId(id) => id,
            _ => panic!("cap id"),
        };
        k.handle(&mut tx, KernelCall::Commit).unwrap();
        assert!(
            k.test_capability_records()
                .iter()
                .any(|x| x.capability_id == cid && x.active),
            "cap must be active after commit"
        );
        holder = h;
        resource = r;
        cap_id = cid;
        // grant_sequence is not on CapabilitySemanticRecord; use graph counter.
        seq = k.capability_sequence();
        snap = snapshot_world(&k);
    }

    let mut delta = empty_delta(90, snap.version + 1);
    delta.capability_grants.push(PendingCapabilityGrant {
        capability_id: cap_id,
        grant_sequence: seq,
        grantor: holder,
        grantee: holder,
        resource,
        cap_type: "read".into(),
    });
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    let active = k2
        .test_capability_records()
        .into_iter()
        .filter(|r| r.capability_id == cap_id && r.active)
        .count();
    eprintln!("duplicate cap grant: active records for cap_id={cap_id}: {active}");
    assert!(
        active <= 2,
        "capability records must not explode, got {active}"
    );
    cleanup(&wal);
}

/// recovery → commit → recovery → commit chain.
#[test]
fn audit_recovery_commit_recovery_chain() {
    let wal = temp_wal("chain");
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
    }
    let v1 = {
        let k = Kernel::with_wal_path(wal.clone());
        let v = k.get_global_version();
        let _ = birth_kernel(&k);
        let v2 = k.get_global_version();
        assert!(v2 > v);
        v2
    };
    let v2 = {
        let k = Kernel::with_wal_path(wal.clone());
        assert_eq!(k.get_global_version(), v1);
        let _ = birth_kernel(&k);
        k.get_global_version()
    };
    let k = Kernel::with_wal_path(wal.clone());
    assert_eq!(k.get_global_version(), v2);
    assert_eq!(k.list_object_ids().len(), 3);
    cleanup(&wal);
}

/// Empty-body TransactionCommitted with advanced version only.
#[test]
fn audit_wal_empty_delta_bumps_version() {
    let wal = temp_wal("empty_delta");
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
        snap = snapshot_world(&k);
    }
    // Inject a version-gap empty delta (current+5). Constitution §5 case D:
    // version > current + 1 must REJECT with zero mutation.
    let delta = empty_delta(70, snap.version + 5);
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    let v = k2.get_global_version();
    eprintln!(
        "empty delta: before_ver={} injected_ver={} after_ver={}",
        snap.version,
        snap.version + 5,
        v
    );
    // Version gap must be rejected: global_version stays at pre-gap value.
    assert_eq!(
        v,
        snap.version,
        "version-gap empty TransactionCommitted must be rejected; global_version must not jump"
    );
    // Objects unchanged.
    assert_eq!(k2.list_object_ids().len(), snap.ids.len());
    cleanup(&wal);
}

/// Valid UTF-8 TXCOMMIT with illegal field values (BIRTH id=0).
#[test]
fn audit_wal_illegal_field_values() {
    let wal = temp_wal("illegal");
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
    }
    let mut delta = empty_delta(60, 10);
    delta.births.push(0); // object id 0
    append_wal_entry(&wal, &WalEntry::TransactionCommitted(delta));

    let k2 = Kernel::with_wal_path(wal.clone());
    // Constitution: ObjectId=0 保留作为"调用者标识"，不是 Object。
    // 所以 BIRTH id=0 应被拒绝或忽略，不应出现在 object registry。
    let ids = k2.list_object_ids();
    assert!(
        !ids.contains(&0),
        "BUG E: illegal BIRTH id=0 produced object 0 in registry: ids={ids:?}"
    );
    let _ = k2.state_root();
    cleanup(&wal);
}

/// Half-valid: corrupt CRC so record is skipped; prior state preserved.
#[test]
fn audit_wal_corrupt_crc_preserves_prior() {
    let wal = temp_wal("crc");
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let _ = birth_kernel(&k);
        let _ = birth_kernel(&k);
        snap = snapshot_world(&k);
    }

    // Append a line with valid-looking structure but wrong CRC.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&wal)
            .unwrap();
        // LEN/CRC mismatch — deserializer returns None → recover stops reading further.
        writeln!(
            f,
            "LEN=40 CRC=deadbeef TXCOMMIT TX=99 VERSION=99 ACTOR=0 BIRTH 12345 END"
        )
        .unwrap();
    }

    let k2 = Kernel::with_wal_path(wal.clone());
    let snap2 = snapshot_world(&k2);
    // Prior objects should remain (CRC fail breaks further reads, does not wipe).
    assert_eq!(
        snap2.ids, snap.ids,
        "CRC failure on trailing line must not destroy prior recovered state"
    );
    assert_eq!(snap2.version, snap.version);
    cleanup(&wal);
}

/// Replay: open same WAL twice; state identical (idempotent recovery).
#[test]
fn audit_wal_replay_committed_delta_idempotent() {
    let wal = temp_wal("replay");
    let snap: WorldSnap;
    {
        let k = Kernel::with_wal_path(wal.clone());
        let a = birth_kernel(&k);
        let mut tx = k.test_begin_in_object(a);
        k.test_write(&mut tx, 0, b"data".to_vec()).unwrap();
        k.handle(&mut tx, KernelCall::Commit).unwrap();
        let b = birth_kernel(&k);
        // link with shared creator path
        let mut tx = k.test_begin_in_object(a);
        // grant self on b then link — or birth b under a
        let _ = b;
        k.handle(&mut tx, KernelCall::Commit).ok();
        snap = snapshot_world(&k);
    }

    let s1 = snapshot_world(&Kernel::with_wal_path(wal.clone()));
    let s2 = snapshot_world(&Kernel::with_wal_path(wal.clone()));
    let s3 = snapshot_world(&Kernel::with_wal_path(wal.clone()));
    assert_eq!(s1, s2, "recovery 1 vs 2");
    assert_eq!(s2, s3, "recovery 2 vs 3");
    assert_eq!(s1.ids, snap.ids);
    assert_eq!(s1.version, snap.version);
    assert_eq!(s1.root, snap.root);
    cleanup(&wal);
}

// =============================================================================
// Part 4: freeze/death still pre-auth (control contrast already above)
// =============================================================================

/// 跨对象 write 在无 capability 时仍被拒绝（审计回归）。
/// 失败意味着 write 授权检查被绕过。
#[test]
fn audit_write_cross_object_still_denied() {
    let (wal, kernel, world) = world_pair("write_x");
    let sid0 = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();
    let sid1 = world.tx_begin(None).unwrap();
    let b = world.tx_create_object(sid1).unwrap();
    world.tx_write(sid1, 0, b"secret".to_vec(), Some(b)).unwrap();
    world.tx_commit(sid1).unwrap();
    let root = kernel.state_root();

    let sid = world.tx_begin(Some(a)).unwrap();
    assert!(world
        .tx_write(sid, 0, b"hack".to_vec(), Some(b))
        .is_err());
    let _ = world.tx_abort(sid);
    assert_eq!(kernel.state_root(), root);
    cleanup(&wal);
}

// =============================================================================
// Part 5: Commit Version & Delta Identity Constitution v0.1 — behavior matrix
//
// Constitution: docs/constitution/commit_version.md
//
// apply() target state machine:
//   version <  current              → REJECT  (atomic, no mutation)
//   version == current && same hash → NO-OP   (idempotent, root/version unchanged)
//   version == current && diff hash → REJECT  (atomic, no mutation)
//   version == current + 1          → APPLY
//   version >  current + 1          → REJECT  (atomic, no mutation)
//
// Production code is NOT modified in this audit. Tests that fail because the
// constitution is not yet implemented are EXPECTED RED.
// =============================================================================

/// Helper: construct a TransactionDelta with explicit fields.
fn make_delta(
    tx_id: u64,
    version: u64,
    actor_id: u64,
    writes: Vec<(Address, Vec<u8>)>,
    births: Vec<u64>,
    deaths: Vec<u64>,
    freezes: Vec<u64>,
    links: Vec<(u64, u64, LinkType)>,
    unlinks: Vec<(u64, u64)>,
    effects: Vec<(String, Vec<u8>)>,
) -> TransactionDelta {
    TransactionDelta {
        tx_id,
        commit_version: version,
        actor_id,
        writes,
        scope_changes: vec![],
        births,
        deaths,
        freezes,
        links,
        unlinks,
        capability_grants: vec![],
        capability_delegates: vec![],
        capability_revokes: vec![],
        effects,
    }
}

/// 1. First apply: current=0, incoming.version=1 → APPLY
#[test]
fn audit_commit_version_first_apply() {
    let (wal, kernel, _world) = world_pair("cv_first");

    assert_eq!(kernel.get_global_version(), 0, "precondition: start at version 0");
    let before = snapshot_world(&kernel);

    // Minimal new delta at version 1: birth object 1
    let delta = make_delta(
        1,
        1,
        0,
        vec![],
        vec![1],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );

    kernel.test_apply(&delta);

    let after = snapshot_world(&kernel);
    assert_eq!(
        after.version, 1,
        "APPLY version 1 must advance global_version to 1"
    );
    assert!(
        after.ids.contains(&1),
        "birth in delta must be visible in World State"
    );
    assert_ne!(
        before.root, after.root,
        "first APPLY must change root_hash (new object)"
    );

    cleanup(&wal);
}

/// 2. Consecutive apply: version 1 then version 2 → both APPLY
#[test]
fn audit_commit_version_consecutive_apply() {
    let (wal, kernel, _world) = world_pair("cv_consec");

    assert_eq!(kernel.get_global_version(), 0);

    let d1 = make_delta(
        10,
        1,
        0,
        vec![],
        vec![10],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d1);
    assert_eq!(kernel.get_global_version(), 1);
    assert!(kernel.list_object_ids().contains(&10));

    let d2 = make_delta(
        20,
        2,
        0,
        vec![(Address::new(10, 0), b"v2-payload".to_vec())],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let root_after_1 = kernel.state_root();
    kernel.test_apply(&d2);

    assert_eq!(
        kernel.get_global_version(),
        2,
        "second consecutive APPLY must advance global_version 1 → 2"
    );
    assert_ne!(
        root_after_1,
        kernel.state_root(),
        "write in version-2 delta must change root_hash"
    );

    cleanup(&wal);
}

/// 3. Equal version + same content → NO-OP (idempotent)
///
/// tx_id is NOT part of Delta Identity; different tx_id with identical
/// canonical content must still be recognized as the same Delta.
#[test]
fn audit_equal_version_same_content_is_idempotent() {
    let (wal, kernel, _world) = world_pair("cv_same_content");

    let d1 = make_delta(
        100,
        1,
        0,
        vec![],
        vec![42],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d1);
    assert_eq!(kernel.get_global_version(), 1);
    let snap_after_apply = snapshot_world(&kernel);
    let root_after_apply = kernel.state_root();

    // Same content, different tx_id, same version
    let d1_replay = make_delta(
        9999, // different tx_id — must not matter
        1,
        0,
        vec![],
        vec![42],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d1_replay);

    let snap_after_noop = snapshot_world(&kernel);
    assert_eq!(
        snap_after_noop.version, 1,
        "NO-OP must leave global_version unchanged"
    );
    assert_eq!(
        kernel.state_root(),
        root_after_apply,
        "NO-OP must leave root_hash completely unchanged"
    );
    assert_eq!(
        snap_after_noop, snap_after_apply,
        "NO-OP must leave all observable World State components unchanged"
    );

    cleanup(&wal);
}

/// 4. Equal version + different content → REJECT (history conflict)
///
/// Upgrade of the former audit_equal_version_residual_gap_red.
/// Core residual-gap contract: same version, different payload must not mutate.
#[test]
fn audit_equal_version_different_content_is_rejected() {
    let (wal, kernel, world) = world_pair("cv_diff_content");

    // Establish version 1 with a known write
    let sid = world.tx_begin(None).unwrap();
    let obj = world.tx_create_object(sid).unwrap();
    world
        .tx_write(sid, 0, b"original-A".to_vec(), Some(obj))
        .unwrap();
    world.tx_commit(sid).unwrap();

    let current = kernel.get_global_version();
    assert!(current >= 1);
    let snap_before = snapshot_world(&kernel);
    let root_before = kernel.state_root();

    // Same version, different content (overwrite state with "B" + extra death attempt)
    let conflict = make_delta(
        7777,
        current, // equal version
        0,
        vec![(Address::new(obj, 0), b"conflict-B".to_vec())],
        vec![],
        vec![obj], // different mutation set
        vec![],
        vec![],
        vec![],
        vec![],
    );

    // apply() is currently void; constitution requires REJECT before any mutation.
    // We verify atomicity via World State identity, not via Result (API may still
    // be void until production implements the gate).
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        kernel.test_apply(&conflict);
    }));

    let snap_after = snapshot_world(&kernel);
    assert_eq!(
        snap_after.version, snap_before.version,
        "REJECT equal-version different content must leave global_version unchanged"
    );
    assert_eq!(
        kernel.state_root(),
        root_before,
        "REJECT must leave root_hash unchanged"
    );
    assert_eq!(
        snap_after, snap_before,
        "REJECT must leave World State (objects/links/caps/root/version) fully identical — no partial mutation"
    );

    // Object must still be alive (death in conflict delta must not have applied)
    assert!(
        kernel.list_object_ids().contains(&obj),
        "object must still exist after rejected conflict delta"
    );

    cleanup(&wal);
}

/// 5. Stale version (version < current) → REJECT
///
/// Even if content hash matched a historical delta, stale version must REJECT.
/// NO-OP is ONLY for equal version + same content hash.
#[test]
fn audit_stale_version_is_rejected() {
    let (wal, kernel, _world) = world_pair("cv_stale");

    // Advance to version 2
    kernel.test_apply(&make_delta(1, 1, 0, vec![], vec![1], vec![], vec![], vec![], vec![], vec![]));
    kernel.test_apply(&make_delta(2, 2, 0, vec![], vec![2], vec![], vec![], vec![], vec![], vec![]));
    assert_eq!(kernel.get_global_version(), 2);

    let snap_before = snapshot_world(&kernel);
    let root_before = kernel.state_root();

    // Stale version 1 (content may even match historical birth of 1)
    let stale = make_delta(
        50,
        1, // < current
        0,
        vec![],
        vec![1],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&stale);

    let snap_after = snapshot_world(&kernel);
    assert_eq!(
        snap_after.version, 2,
        "stale version must not change global_version"
    );
    assert_eq!(
        kernel.state_root(),
        root_before,
        "stale version REJECT must leave root_hash unchanged"
    );
    assert_eq!(
        snap_after, snap_before,
        "stale version REJECT must leave World State fully identical"
    );

    cleanup(&wal);
}

/// 6. Version gap (version > current + 1) → REJECT
#[test]
fn audit_version_gap_is_rejected() {
    let (wal, kernel, _world) = world_pair("cv_gap");

    kernel.test_apply(&make_delta(1, 1, 0, vec![], vec![1], vec![], vec![], vec![], vec![], vec![]));
    kernel.test_apply(&make_delta(2, 2, 0, vec![], vec![2], vec![], vec![], vec![], vec![], vec![]));
    assert_eq!(kernel.get_global_version(), 2);

    let snap_before = snapshot_world(&kernel);
    let root_before = kernel.state_root();

    // Skip version 3 → jump to 4
    let gap = make_delta(
        40,
        4,
        0,
        vec![],
        vec![99],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        kernel.test_apply(&gap);
    }));

    let snap_after = snapshot_world(&kernel);
    assert_eq!(
        snap_after.version, 2,
        "version gap must leave global_version at 2"
    );
    assert_eq!(
        kernel.state_root(),
        root_before,
        "version gap REJECT must leave root_hash unchanged"
    );
    assert_eq!(
        snap_after, snap_before,
        "version gap REJECT must leave World State fully identical"
    );
    assert!(
        !kernel.list_object_ids().contains(&99),
        "birth in gapped delta must not have been applied"
    );

    cleanup(&wal);
}

/// 7. Repeated WAL replay is idempotent (A then B, then A then B again → NO-OP)
#[test]
fn audit_repeated_wal_replay_is_idempotent() {
    let (wal, kernel, _world) = world_pair("cv_replay");

    let a = make_delta(1, 1, 0, vec![], vec![11], vec![], vec![], vec![], vec![], vec![]);
    let b = make_delta(
        2,
        2,
        0,
        vec![(Address::new(11, 0), b"replay-B".to_vec())],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );

    // First replay
    kernel.test_apply(&a);
    kernel.test_apply(&b);
    assert_eq!(kernel.get_global_version(), 2);
    let snap_first = snapshot_world(&kernel);
    let root_first = kernel.state_root();

    // Second replay of the same sequence
    kernel.test_apply(&a); // equal version 1 + same content → NO-OP
    kernel.test_apply(&b); // equal version 2 + same content → NO-OP

    let snap_second = snapshot_world(&kernel);
    assert_eq!(
        snap_second.version, 2,
        "repeated replay must leave global_version at 2"
    );
    assert_eq!(
        kernel.state_root(),
        root_first,
        "repeated replay must leave root_hash identical to first replay"
    );
    assert_eq!(
        snap_second, snap_first,
        "repeated WAL replay must be fully idempotent on World State"
    );

    cleanup(&wal);
}

/// 8. REJECT is atomic — multi-mutation illegal delta must not partially apply
#[test]
fn audit_rejected_delta_is_atomic() {
    let (wal, kernel, _world) = world_pair("cv_atomic");

    // Known baseline: version 1 with object 7
    kernel.test_apply(&make_delta(1, 1, 0, vec![], vec![7], vec![], vec![], vec![], vec![], vec![]));
    assert_eq!(kernel.get_global_version(), 1);

    let snap_before = snapshot_world(&kernel);
    let root_before = kernel.state_root();
    let ids_before = kernel.list_object_ids();

    // Illegal: version 0 (stale) but rich mutation set
    let illegal = make_delta(
        888,
        0, // stale → must REJECT before any mutation
        0,
        vec![(Address::new(7, 0), b"should-not-write".to_vec())],
        vec![100, 101], // births
        vec![7],        // death
        vec![7],        // freeze
        vec![(7, 100, LinkType::References)],
        vec![],
        vec![("eff-key".into(), b"payload".to_vec())],
    );

    kernel.test_apply(&illegal);

    let snap_after = snapshot_world(&kernel);
    assert_eq!(
        snap_after, snap_before,
        "REJECT must be atomic: full WorldSnap identity"
    );
    assert_eq!(kernel.state_root(), root_before);
    assert_eq!(kernel.get_global_version(), 1);
    assert_eq!(kernel.list_object_ids(), ids_before);
    assert!(
        !kernel.list_object_ids().contains(&100),
        "birth must not partially apply on REJECT"
    );
    assert!(
        kernel.list_object_ids().contains(&7),
        "death must not partially apply on REJECT"
    );

    cleanup(&wal);
}

/// 9. Equal version + same content preserves root identity (NO-OP root pin)
#[test]
fn audit_equal_version_same_content_preserves_root() {
    let (wal, kernel, _world) = world_pair("cv_root_pin");

    let d = make_delta(
        55,
        1,
        0,
        vec![(Address::new(0, 0), b"pin-value".to_vec())],
        vec![55],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d);
    let root_before = kernel.state_root();
    let snap_before = snapshot_world(&kernel);
    assert_eq!(kernel.get_global_version(), 1);

    // Replay identical content, different tx_id
    let d_again = make_delta(
        66,
        1,
        0,
        vec![(Address::new(0, 0), b"pin-value".to_vec())],
        vec![55],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d_again);

    assert_eq!(
        kernel.state_root(),
        root_before,
        "NO-OP same-content equal-version must preserve root_hash exactly"
    );
    assert_eq!(kernel.get_global_version(), 1);
    assert_eq!(
        snapshot_world(&kernel),
        snap_before,
        "NO-OP must preserve full World State"
    );

    cleanup(&wal);
}

// Note on actor_id identity:
// actor_id belongs to Delta content (canonical_identity_bytes).
// Once canonical_identity_bytes() is implemented, add a test that
// same version + same mutations + different actor_id → different
// content hash → REJECT (not NO-OP). Current production API has no
// observable content-hash entry point; do not invent a hash here.

// =============================================================================
// Canonical Identity & Checkpoint Continuity (this round infrastructure)
// =============================================================================

fn base_delta() -> TransactionDelta {
    TransactionDelta {
        tx_id: 1,
        commit_version: 1,
        actor_id: 42,
        writes: vec![(Address::new(1, 1), b"v".to_vec())],
        scope_changes: vec![(1, ScopeChangeType::Bind, 10)],
        births: vec![1, 2],
        deaths: vec![3],
        freezes: vec![4],
        links: vec![(1, 2, LinkType::Owns)],
        unlinks: vec![(5, 6)],
        capability_grants: vec![PendingCapabilityGrant {
            capability_id: 100,
            grant_sequence: 1,
            cap_type: "Admin".to_string(),
            grantor: 1,
            grantee: 2,
            resource: 3,
        }],
        capability_delegates: vec![PendingCapabilityDelegate {
            capability_id: 100,
            from: 2,
            to: 7,
            cascade_on_revoke: true,
        }],
        capability_revokes: vec![PendingCapabilityRevoke {
            capability_id: 100,
            holder: 7,
            cascade_override: Some(true),
        }],
        effects: vec![("k".to_string(), b"p".to_vec())],
    }
}

/// 相同 delta 的 canonical identity 相等。
/// 失败意味着 identity 计算非确定性或字段遗漏。
#[test]
fn audit_canonical_identity_same_delta_equal() {
    let a = base_delta();
    let b = base_delta();
    assert_eq!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "identical deltas must produce identical canonical bytes"
    );
    assert_eq!(a.content_hash(), b.content_hash());
}

/// Canonical identity 排除 tx_id（非语义字段）。
/// 失败意味着 identity 混入了瞬时字段。
#[test]
fn audit_canonical_identity_excludes_tx_id() {
    let mut a = base_delta();
    let mut b = base_delta();
    a.tx_id = 1;
    b.tx_id = 999;
    assert_eq!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "tx_id must not participate in canonical identity"
    );
}

/// Canonical identity 排除 commit_version。
/// 失败意味着 identity 混入了版本瞬时字段。
#[test]
fn audit_canonical_identity_excludes_commit_version() {
    let mut a = base_delta();
    let mut b = base_delta();
    a.commit_version = 1;
    b.commit_version = 99;
    assert_eq!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "commit_version must not participate in canonical identity"
    );
}

/// Canonical identity 包含 actor_id。
/// 失败意味着归因信息丢失。
#[test]
fn audit_canonical_identity_includes_actor_id() {
    let mut a = base_delta();
    let mut b = base_delta();
    a.actor_id = 1;
    b.actor_id = 2;
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "actor_id must participate in canonical identity"
    );
}

/// Vec 字段顺序影响 canonical identity。
/// 失败意味着顺序被错误规范化或忽略。
#[test]
fn audit_canonical_identity_vec_order_matters() {
    let mut a = base_delta();
    let mut b = base_delta();
    a.births = vec![1, 2];
    b.births = vec![2, 1];
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "Vec order is semantic; births [1,2] != [2,1]"
    );
}

/// String 边界在 canonical identity 中安全（无歧义）。
/// 失败意味着编码存在边界碰撞。
#[test]
fn audit_canonical_identity_string_boundary_safe() {
    let mut a = empty_delta(1, 1);
    let mut b = empty_delta(1, 1);
    a.effects = vec![("ab".to_string(), b"c".to_vec())];
    b.effects = vec![("a".to_string(), b"bc".to_vec())];
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "string/byte boundary must be unambiguous"
    );

    let mut c = empty_delta(1, 1);
    c.effects = vec![
        ("a".to_string(), b"b".to_vec()),
        ("c".to_string(), vec![]),
    ];
    assert_ne!(a.canonical_identity_bytes(), c.canonical_identity_bytes());
}

/// 空 Vec 与非空 Vec 的 identity 不同。
/// 失败意味着空集合被错误处理。
#[test]
fn audit_canonical_identity_empty_vs_nonempty_vec() {
    let mut a = empty_delta(1, 1);
    let mut b = empty_delta(1, 1);
    a.births = vec![];
    b.births = vec![1];
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "empty Vec must not collide with non-empty"
    );
}

/// Option Some/None 在 identity 中可区分。
/// 失败意味着 Option 编码丢失信息。
#[test]
fn audit_canonical_identity_option_some_none() {
    let mut a = empty_delta(1, 1);
    let mut b = empty_delta(1, 1);
    a.capability_revokes = vec![PendingCapabilityRevoke {
        capability_id: 1,
        holder: 2,
        cascade_override: None,
    }];
    b.capability_revokes = vec![PendingCapabilityRevoke {
        capability_id: 1,
        holder: 2,
        cascade_override: Some(true),
    }];
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "Option None vs Some must differ"
    );
    let mut c = empty_delta(1, 1);
    c.capability_revokes = vec![PendingCapabilityRevoke {
        capability_id: 1,
        holder: 2,
        cascade_override: Some(false),
    }];
    assert_ne!(b.canonical_identity_bytes(), c.canonical_identity_bytes());
    assert_ne!(a.canonical_identity_bytes(), c.canonical_identity_bytes());
}

/// Enum 不同 variant 产生不同 identity。
/// 失败意味着 variant 区分失败。
#[test]
fn audit_canonical_identity_enum_variants() {
    let mut a = empty_delta(1, 1);
    let mut b = empty_delta(1, 1);
    a.scope_changes = vec![(1, ScopeChangeType::Bind, 10)];
    b.scope_changes = vec![(1, ScopeChangeType::Unbind, 10)];
    assert_ne!(
        a.canonical_identity_bytes(),
        b.canonical_identity_bytes(),
        "enum variants must produce different identity"
    );

    let mut c = empty_delta(1, 1);
    let mut d = empty_delta(1, 1);
    c.links = vec![(1, 2, LinkType::Owns)];
    d.links = vec![(1, 2, LinkType::DependsOn)];
    assert_ne!(c.canonical_identity_bytes(), d.canonical_identity_bytes());
}

/// 每个语义字段都影响 canonical identity。
/// 失败意味着某些语义字段被遗漏。
#[test]
fn audit_canonical_identity_every_semantic_field() {
    let empty = empty_delta(1, 1);
    let base = empty.canonical_identity_bytes();

    let cases: Vec<(&str, TransactionDelta)> = vec![
        ("actor_id", {
            let mut d = empty_delta(1, 1);
            d.actor_id = 7;
            d
        }),
        ("writes", {
            let mut d = empty_delta(1, 1);
            d.writes = vec![(Address::new(1, 1), b"x".to_vec())];
            d
        }),
        ("scope_changes", {
            let mut d = empty_delta(1, 1);
            d.scope_changes = vec![(1, ScopeChangeType::Bind, 1)];
            d
        }),
        ("births", {
            let mut d = empty_delta(1, 1);
            d.births = vec![9];
            d
        }),
        ("deaths", {
            let mut d = empty_delta(1, 1);
            d.deaths = vec![9];
            d
        }),
        ("freezes", {
            let mut d = empty_delta(1, 1);
            d.freezes = vec![9];
            d
        }),
        ("links", {
            let mut d = empty_delta(1, 1);
            d.links = vec![(1, 2, LinkType::References)];
            d
        }),
        ("unlinks", {
            let mut d = empty_delta(1, 1);
            d.unlinks = vec![(1, 2)];
            d
        }),
        ("capability_grants", {
            let mut d = empty_delta(1, 1);
            d.capability_grants = vec![PendingCapabilityGrant {
                capability_id: 1,
                grant_sequence: 1,
                cap_type: "T".into(),
                grantor: 1,
                grantee: 2,
                resource: 3,
            }];
            d
        }),
        ("capability_delegates", {
            let mut d = empty_delta(1, 1);
            d.capability_delegates = vec![PendingCapabilityDelegate {
                capability_id: 1,
                from: 1,
                to: 2,
                cascade_on_revoke: false,
            }];
            d
        }),
        ("capability_revokes", {
            let mut d = empty_delta(1, 1);
            d.capability_revokes = vec![PendingCapabilityRevoke {
                capability_id: 1,
                holder: 2,
                cascade_override: None,
            }];
            d
        }),
        ("effects", {
            let mut d = empty_delta(1, 1);
            d.effects = vec![("e".into(), b"1".to_vec())];
            d
        }),
    ];

    for (name, d) in cases {
        assert_ne!(
            d.canonical_identity_bytes(),
            base,
            "field `{name}` must affect canonical identity"
        );
    }

    let mut tx_only = empty_delta(1, 1);
    tx_only.tx_id = 12345;
    assert_eq!(tx_only.canonical_identity_bytes(), base);
    let mut ver_only = empty_delta(1, 1);
    ver_only.commit_version = 77;
    assert_eq!(ver_only.canonical_identity_bytes(), base);
}

/// Genesis 时 last_applied_delta_hash 为零。
/// 失败意味着初始 hash 状态错误。
#[test]
fn audit_last_applied_delta_hash_genesis_is_zero() {
    let (wal, kernel, _world) = world_pair("lah_genesis");
    assert_eq!(
        kernel.get_last_applied_delta_hash(),
        ZERO_HASH,
        "genesis last_applied_delta_hash must be ZERO_HASH"
    );
    assert_eq!(kernel.get_global_version(), 0);
    cleanup(&wal);
}

/// Apply delta 后 last_applied_delta_hash 更新。
/// 失败意味着 hash 链未推进。
#[test]
fn audit_last_applied_delta_hash_updates_on_apply() {
    let (wal, kernel, _world) = world_pair("lah_apply");
    let d = make_delta(1, 1, 0, vec![], vec![1], vec![], vec![], vec![], vec![], vec![]);
    let expected = d.content_hash();
    kernel.test_apply(&d);
    assert_eq!(
        kernel.get_last_applied_delta_hash(),
        expected,
        "successful apply must record delta content_hash"
    );
    cleanup(&wal);
}

/// Checkpoint 保留 last_applied_delta_hash。
/// 失败意味着 hash 链在 checkpoint 丢失。
#[test]
fn audit_checkpoint_preserves_last_applied_delta_hash() {
    let (wal, kernel, _world) = world_pair("lah_ckpt");
    let d = make_delta(1, 1, 5, vec![], vec![11], vec![], vec![], vec![], vec![], vec![]);
    kernel.test_apply(&d);
    let h = kernel.get_last_applied_delta_hash();
    assert_ne!(h, ZERO_HASH);

    let snap = kernel.create_checkpoint();
    assert_eq!(
        snap.last_applied_delta_hash, h,
        "create_checkpoint must persist last_applied_delta_hash"
    );

    let d2 = make_delta(2, 2, 5, vec![], vec![12], vec![], vec![], vec![], vec![], vec![]);
    kernel.test_apply(&d2);
    assert_ne!(kernel.get_last_applied_delta_hash(), h);

    assert!(kernel.restore_checkpoint(&snap));
    assert_eq!(
        kernel.get_last_applied_delta_hash(),
        h,
        "restore_checkpoint must restore last_applied_delta_hash"
    );
    assert_eq!(kernel.get_global_version(), 1);
    cleanup(&wal);
}

/// Checkpoint roundtrip 后 identity 连续。
/// 失败意味着 identity 在持久化路径上漂移。
#[test]
fn audit_checkpoint_roundtrip_identity_continuity() {
    let (wal, kernel, _world) = world_pair("lah_roundtrip");
    let d = make_delta(1, 1, 9, vec![], vec![21], vec![], vec![], vec![], vec![], vec![]);
    kernel.test_apply(&d);
    let hash_a = kernel.get_last_applied_delta_hash();
    let ver_a = kernel.get_global_version();
    let root_a = kernel.test_engine().root_hash();

    let snap = kernel.create_checkpoint();

    let d2 = make_delta(2, 2, 9, vec![], vec![22], vec![], vec![], vec![], vec![], vec![]);
    kernel.test_apply(&d2);
    assert_ne!(kernel.get_last_applied_delta_hash(), hash_a);

    assert!(kernel.restore_checkpoint(&snap));
    assert_eq!(kernel.get_global_version(), ver_a);
    assert_eq!(kernel.get_last_applied_delta_hash(), hash_a);
    assert_eq!(kernel.test_engine().root_hash(), root_a);
    assert!(kernel.list_object_ids().contains(&21));
    assert!(!kernel.list_object_ids().contains(&22));
    cleanup(&wal);
}
