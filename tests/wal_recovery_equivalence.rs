use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use veritas_kernel::engine::VeritasEngine;
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
fn assert_recovery_equivalence(operations: &[&dyn Fn(&VeritasEngine)]) {
    let wal_path = unique_wal_path();
    let _ = std::fs::remove_file(&wal_path);

    let snapshot_before;
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        for op in operations {
            op(&engine);
        }
        snapshot_before = EngineSnapshot::capture(&engine);
    } // crash

    {
        let recovered = VeritasEngine::with_wal_path(wal_path.clone());
        let snapshot_after = EngineSnapshot::capture(&recovered);

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

fn commit_birth(engine: &VeritasEngine, id: u64) {
    let mut tx = engine.begin();
    engine.object_birth(&mut tx, id).unwrap();
    engine.commit(&mut tx).unwrap();
}

fn commit_link(engine: &VeritasEngine, from: u64, to: u64, lt: LinkType) {
    let mut tx = engine.begin();
    engine.object_link(&mut tx, from, to, lt).unwrap();
    engine.commit(&mut tx).unwrap();
}

fn commit_death(engine: &VeritasEngine, id: u64) {
    let mut tx = engine.begin();
    engine.object_death(&mut tx, id).unwrap();
    engine.commit(&mut tx).unwrap();
}

fn commit_freeze(engine: &VeritasEngine, id: u64) {
    let mut tx = engine.begin();
    engine.object_freeze(&mut tx, id).unwrap();
    engine.commit(&mut tx).unwrap();
}

/// P29.3: Single object birth → recovery equivalence
#[test]
fn equivalence_single_birth() {
    assert_recovery_equivalence(&[
        &|e| commit_birth(e, 1),
    ]);
}

/// P29.3: Two objects + link → recovery equivalence
#[test]
fn equivalence_birth_and_link() {
    assert_recovery_equivalence(&[
        &|e| commit_birth(e, 10),
        &|e| commit_birth(e, 20),
        &|e| commit_link(e, 10, 20, LinkType::Owns),
    ]);
}

/// P29.3: Birth → freeze → death → recovery equivalence
#[test]
fn equivalence_full_lifecycle() {
    assert_recovery_equivalence(&[
        &|e| commit_birth(e, 100),
        &|e| commit_freeze(e, 100),
        &|e| commit_death(e, 100),
    ]);
}

/// P29.3: Multi-object topology → recovery equivalence
#[test]
fn equivalence_multi_object_topology() {
    assert_recovery_equivalence(&[
        &|e| commit_birth(e, 1),
        &|e| commit_birth(e, 2),
        &|e| commit_birth(e, 3),
        &|e| commit_link(e, 1, 2, LinkType::Owns),
        &|e| commit_link(e, 1, 3, LinkType::DependsOn),
        &|e| commit_link(e, 2, 3, LinkType::References),
    ]);
}

/// P29.3: Death cascade → recovery equivalence
#[test]
fn equivalence_death_cascade() {
    assert_recovery_equivalence(&[
        &|e| commit_birth(e, 1),
        &|e| commit_birth(e, 2),
        &|e| commit_birth(e, 3),
        &|e| commit_link(e, 1, 2, LinkType::Owns),
        &|e| commit_link(e, 2, 3, LinkType::Owns),
        &|e| commit_death(e, 1), // cascade kills 2 and 3
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
    let engine = VeritasEngine::with_wal_path(wal_path.clone());
    commit_birth(&engine, 1); // A
    commit_birth(&engine, 2); // B
    {
        let mut tx = engine.begin();
        engine.object_link(&mut tx, 1, 2, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
    }
    {
        let mut tx = engine.begin();
        engine.object_unlink(&mut tx, 1, 2).unwrap();
        engine.commit(&mut tx).unwrap();
    }
    commit_death(&engine, 1); // kill A
    drop(engine); // crash

    // Recovery
    let recovered = VeritasEngine::with_wal_path(wal_path.clone());
    assert!(
        recovered.is_object_dead(1),
        "A should be dead after recovery"
    );
    assert!(
        !recovered.is_object_dead(2),
        "B must survive: unlinked before A's death"
    );
    assert!(
        !recovered.has_link(1, 2),
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
    let engine = VeritasEngine::with_wal_path(wal_path.clone());
    commit_birth(&engine, 1); // A
    commit_birth(&engine, 2); // B
    {
        let mut tx = engine.begin();
        engine.object_link(&mut tx, 1, 2, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
    }
    commit_death(&engine, 1); // kill A → cascade to B
    drop(engine); // crash

    // Recovery
    let recovered = VeritasEngine::with_wal_path(wal_path.clone());
    assert!(recovered.is_object_dead(1), "A should be dead");
    assert!(
        recovered.is_object_dead(2),
        "B must be dead: linked when A died, cascade applies"
    );
    assert!(
        !recovered.has_link(1, 2),
        "A->B link must be gone after cascade cleanup"
    );
    let _ = std::fs::remove_file(&wal_path);
}
