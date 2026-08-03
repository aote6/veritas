use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::ObjectType;
use veritas_kernel::types::{ObjectState, LinkType};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
fn unique_wal_path() -> String {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("target/test_eq_{}_{}.wal", std::process::id(), id)
}

/// Collect a complete snapshot of engine state for equivalence comparison.
struct EngineSnapshot {
    object_ids: HashSet<u64>,
    object_states: Vec<(u64, ObjectState)>,
    links: Vec<(u64, u64)>,
    state_root: u64,
}

impl EngineSnapshot {
    fn capture(engine: &VeritasEngine) -> Self {
        let object_ids: HashSet<u64> = engine.list_object_ids().into_iter().collect();
        let mut object_states: Vec<(u64, ObjectState)> = object_ids
            .iter()
            .filter_map(|&id| {
                engine.get_object_state(id).map(|s| (id, s))
            })
            .collect();
        object_states.sort_by_key(|(id, _)| *id);

        let mut links = Vec::new();
        for &from in &object_ids {
            for &to in &object_ids {
                if from != to && engine.has_link(from, to) {
                    links.push((from, to));
                }
            }
        }
        links.sort();

        let state_root = engine.state_root();

        EngineSnapshot {
            object_ids,
            object_states,
            links,
            state_root,
        }
    }
}

/// P29.3: Recovery(WAL) must produce the same engine state as the
/// engine that wrote the WAL before crash. This is the strongest
/// recovery invariant.
fn assert_recovery_equivalence(operations: &[&dyn Fn(&Kernel)]) {
    let wal_path = unique_wal_path();
    let _ = std::fs::remove_file(&wal_path);

    let snapshot_before;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        for op in operations {
            op(&kernel);
        }
        snapshot_before = EngineSnapshot::capture(kernel.engine());
    } // crash

    {
        let recovered = Kernel::with_wal_path(wal_path.clone());
        let snapshot_after = EngineSnapshot::capture(recovered.engine());

        assert_eq!(
            snapshot_after.object_ids, snapshot_before.object_ids,
            "recovered object_ids must match"
        );
        assert_eq!(
            snapshot_after.object_states, snapshot_before.object_states,
            "recovered object states must match"
        );
        assert_eq!(
            snapshot_after.links, snapshot_before.links,
            "recovered links must match"
        );
        assert_eq!(
            snapshot_after.state_root, snapshot_before.state_root,
            "recovered state_root must match"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

fn commit_birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.begin();
    let id = match kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).unwrap() {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

fn commit_link(kernel: &Kernel, from: u64, to: u64, lt: LinkType) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectLink { from, to, link_type: lt }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn commit_death(kernel: &Kernel, id: u64) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectDeath { object_id: id }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn commit_freeze(kernel: &Kernel, id: u64) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectFreeze { object_id: id }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

/// P29.3: Single object birth → recovery equivalence
#[test]
fn equivalence_single_birth() {
    assert_recovery_equivalence(&[
        &|e| { commit_birth(e); },
    ]);
}

/// P29.3: Two objects + link → recovery equivalence
#[test]
fn equivalence_birth_and_link() {
    assert_recovery_equivalence(&[
        &|e| { let a = commit_birth(e); let b = commit_birth(e); commit_link(e, a, b, LinkType::Owns); },
    ]);
}

/// P29.3: Birth → freeze → death → recovery equivalence
#[test]
fn equivalence_full_lifecycle() {
    assert_recovery_equivalence(&[
        &|e| { let id = commit_birth(e); commit_freeze(e, id); commit_death(e, id); },
    ]);
}

/// P29.3: Multi-object topology → recovery equivalence
#[test]
fn equivalence_multi_object_topology() {
    assert_recovery_equivalence(&[
        &|e| { let a = commit_birth(e); let b = commit_birth(e); let c = commit_birth(e); commit_link(e, a, b, LinkType::Owns); commit_link(e, a, c, LinkType::DependsOn); commit_link(e, b, c, LinkType::References); },
    ]);
}

/// P29.3: Death cascade → recovery equivalence
#[test]
fn equivalence_death_cascade() {
    assert_recovery_equivalence(&[
        &|e| { let a = commit_birth(e); let b = commit_birth(e); let c = commit_birth(e); commit_link(e, a, b, LinkType::Owns); commit_link(e, b, c, LinkType::Owns); commit_death(e, a); },
    ]);
}

/// Cross-tx unlink-then-death: tx1 link, tx2 unlink, tx3 death.
/// Recovery must NOT cascade to the unlinked target — topology
/// at death time reflects the unlink, so OWNS closure must not
/// include the previously-owned object.
#[test]
fn cross_tx_unlink_then_death_no_cascade() {
    let wal_path = format!(
        "target/test_unlink_death_{}_{}.wal",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let _ = std::fs::remove_file(&wal_path);

    // tx1: create A and B, link A --OWNS--> B
    // tx2: unlink A --OWNS--> B
    // tx3: kill A
    // Expect: B is still alive (unlinked before death)
    let kernel = Kernel::with_wal_path(wal_path.clone());
    let a = commit_birth(&kernel); // A
    let b = commit_birth(&kernel); // B
    {
        let mut tx = kernel.begin();
        kernel.handle(&mut tx, KernelCall::ObjectLink { from: a, to: b, link_type: LinkType::Owns }).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }
    {
        let mut tx = kernel.begin();
        kernel.handle(&mut tx, KernelCall::ObjectUnlink { from: a, to: b }).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }
    commit_death(&kernel, a); // kill A
    drop(kernel); // crash

    // Recovery
    let recovered = Kernel::with_wal_path(wal_path.clone());
    assert!(
        recovered.engine().is_object_dead(a),
        "A should be dead after recovery"
    );
    assert!(
        !recovered.engine().is_object_dead(b),
        "B must survive: unlinked before A's death"
    );
    assert!(
        !recovered.engine().has_link(a, b),
        "A->B link must be gone after unlink + death cleanup"
    );
    let _ = std::fs::remove_file(&wal_path);
}

/// Same as above, but OWNS link is never unlinked.
/// Death of A must cascade to B.
#[test]
fn cross_tx_link_then_death_cascade() {
    let wal_path = format!(
        "target/test_link_death_{}_{}.wal",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let _ = std::fs::remove_file(&wal_path);

    // tx1: create A and B, link A --OWNS--> B
    // tx2: kill A
    // Expect: both A and B dead
    let kernel = Kernel::with_wal_path(wal_path.clone());
    let a = commit_birth(&kernel); // A
    let b = commit_birth(&kernel); // B
    {
        let mut tx = kernel.begin();
        kernel.handle(&mut tx, KernelCall::ObjectLink { from: a, to: b, link_type: LinkType::Owns }).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }
    commit_death(&kernel, a); // kill A → cascade to B
    drop(kernel); // crash

    // Recovery
    let recovered = Kernel::with_wal_path(wal_path.clone());
    assert!(recovered.engine().is_object_dead(a), "A should be dead");
    assert!(
        recovered.engine().is_object_dead(b),
        "B must be dead: linked when A died, cascade applies"
    );
    assert!(
        !recovered.engine().has_link(a, b),
        "A->B link must be gone after cascade cleanup"
    );
    let _ = std::fs::remove_file(&wal_path);
}
