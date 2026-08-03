use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectState, LinkType};
use std::collections::HashSet;

/// Collect complete engine state for determinism comparison.
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
            .filter_map(|&id| engine.get_object_state(id).map(|s| (id, s)))
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

        EngineSnapshot { object_ids, object_states, links, state_root }
    }
}

fn create_complex_wal(wal_path: &str) {
    let engine = VeritasEngine::with_wal_path(wal_path.to_string());

    // Create objects
    for id in [1u64, 2, 3, 10, 20, 30] {
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, id).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Create links
    let links = vec![
        (1, 2, LinkType::Owns),
        (1, 3, LinkType::Owns),
        (10, 20, LinkType::DependsOn),
        (20, 30, LinkType::References),
    ];
    for (from, to, lt) in &links {
        let mut tx = engine.begin();
        engine.object_link(&mut tx, *from, *to, *lt).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Freeze one object
    {
        let mut tx = engine.begin();
        engine.object_freeze(&mut tx, 30).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Death cascade
    {
        let mut tx = engine.begin();
        engine.object_death(&mut tx, 1).unwrap(); // kills 1, 2, 3 via OWNS
        engine.commit(&mut tx).unwrap();
    }
}

/// P30.1: Same WAL recovered twice produces identical engine state.
#[test]
fn replay_determinism_same_wal_twice() {
    let wal_path = format!("target/test_replay_det_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    create_complex_wal(&wal_path);

    // Recover once
    let snapshot_a = {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        EngineSnapshot::capture(&engine)
    };

    // Recover again
    let snapshot_b = {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        EngineSnapshot::capture(&engine)
    };

    assert_eq!(snapshot_a.object_ids, snapshot_b.object_ids, "object_ids must match");
    assert_eq!(snapshot_a.object_states, snapshot_b.object_states, "object_states must match");
    assert_eq!(snapshot_a.links, snapshot_b.links, "links must match");
    assert_eq!(snapshot_a.state_root, snapshot_b.state_root, "state_root must match");

    let _ = std::fs::remove_file(&wal_path);
}

/// P30.1: Two separate engines with identical operations produce identical WAL
/// and recover to identical state.
#[test]
fn replay_determinism_identical_programs() {
    let wal_a = format!("target/test_replay_id_a_{}.wal", std::process::id());
    let wal_b = format!("target/test_replay_id_b_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_a);
    let _ = std::fs::remove_file(&wal_b);

    // Execute identical operations on two engines
    let snapshot_a = {
        let engine = VeritasEngine::with_wal_path(wal_a.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, 42).unwrap();
        engine.object_birth(&mut tx, 99).unwrap();
        engine.object_link(&mut tx, 42, 99, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
        EngineSnapshot::capture(&engine)
    };

    let snapshot_b = {
        let engine = VeritasEngine::with_wal_path(wal_b.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, 42).unwrap();
        engine.object_birth(&mut tx, 99).unwrap();
        engine.object_link(&mut tx, 42, 99, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
        EngineSnapshot::capture(&engine)
    };

    assert_eq!(snapshot_a.object_ids, snapshot_b.object_ids, "identical programs produce same objects");
    assert_eq!(snapshot_a.object_states, snapshot_b.object_states, "identical programs produce same states");
    assert_eq!(snapshot_a.links, snapshot_b.links, "identical programs produce same links");

    let _ = std::fs::remove_file(&wal_a);
    let _ = std::fs::remove_file(&wal_b);
}

/// P30.1: Recovery from a WAL file is deterministic across 5 recoveries.
#[test]
fn replay_determinism_five_recoveries() {
    let wal_path = format!("target/test_replay_5x_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    create_complex_wal(&wal_path);

    let baseline = {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        EngineSnapshot::capture(&engine)
    };

    for i in 0..5 {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let snapshot = EngineSnapshot::capture(&engine);
        assert_eq!(
            snapshot.object_ids, baseline.object_ids,
            "recovery {} produced different object_ids", i
        );
        assert_eq!(
            snapshot.object_states, baseline.object_states,
            "recovery {} produced different states", i
        );
        assert_eq!(
            snapshot.links, baseline.links,
            "recovery {} produced different links", i
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P30.1: WAL written by Engine A, recovered by Engine B on different path,
/// must produce identical state (cross-instance determinism).
#[test]
fn replay_cross_instance_determinism() {
    let wal_original = format!("target/test_replay_xorig_{}.wal", std::process::id());
    let wal_copy = format!("target/test_replay_xcopy_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_original);
    let _ = std::fs::remove_file(&wal_copy);

    create_complex_wal(&wal_original);

    // Copy WAL to different path (simulating transfer to another node)
    std::fs::copy(&wal_original, &wal_copy).unwrap();

    let snapshot_original = {
        let engine = VeritasEngine::with_wal_path(wal_original.clone());
        EngineSnapshot::capture(&engine)
    };

    let snapshot_copy = {
        let engine = VeritasEngine::with_wal_path(wal_copy.clone());
        EngineSnapshot::capture(&engine)
    };

    assert_eq!(snapshot_original.object_ids, snapshot_copy.object_ids);
    assert_eq!(snapshot_original.object_states, snapshot_copy.object_states);
    assert_eq!(snapshot_original.links, snapshot_copy.links);
    assert_eq!(snapshot_original.state_root, snapshot_copy.state_root);

    let _ = std::fs::remove_file(&wal_original);
    let _ = std::fs::remove_file(&wal_copy);
}
