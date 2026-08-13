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
    AccessIntent, LinkType, ObjectType, PendingCapabilityGrant, TransactionDelta, VeritasError,
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
    // apply stores delta.commit_version — expect jump.
    assert_eq!(
        v,
        snap.version + 5,
        "empty TransactionCommitted still updates global_version via apply"
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
// Part 5: P4 Residual Gap Audit (equal-version different payload exploit)
// =============================================================================

#[test]
fn audit_equal_version_residual_gap_red() {
    let (wal, kernel, world) = world_pair("gap_red");
    
    // 1. 提交正常事务，拉高全局版本号
    let sid = world.tx_begin(None).unwrap();
    let obj_a = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();

    let current_version = kernel.test_engine().get_global_version();
    assert!(current_version > 0);

    // 2. 构造一个同 Version 但含有不同 payload (伪造 DEATH) 的 Delta 文本
    let malicious_payload = format!(
        "TXCOMMIT TX=9999 VERSION={} ACTOR=0 DEATH={} END",
        current_version, obj_a
    );

    let malicious_delta = TransactionDelta::deserialize(&malicious_payload)
        .expect("Failed to deserialize crafted delta text");

    // 3. 尝试重放/应用该 Delta
    let apply_result = std::panic::catch_unwind(|| {
        kernel.test_apply(&malicious_delta);
    });

    // 预期安全行为：同 version 不同内容必须被拒绝 (Err/Panic)
    // 现状 (Gap)：1191 行只检查 delta.commit_version < current，导致同版本直接穿透并杀死对象
    assert!(
        apply_result.is_err(),
        "RED TEST FAILED: Engine accepted crafted Delta with equal version ({}) and different payload!",
        current_version
    );

    cleanup(&wal);
}
