use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectState, LinkType};

/// P29.1: Object created and committed must survive WAL recovery.
/// This is the minimal replay test: birth → crash → recover → verify.
#[test]
fn object_birth_survives_recovery() {
    let wal_path = format!("target/test_obj_birth_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let object_id: u64 = 42;

    // Phase 1: create object
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, object_id).unwrap();
        engine.commit(&mut tx).unwrap();
        assert_eq!(
            engine.get_object_state(object_id),
            Some(ObjectState::Alive),
            "object should be alive after commit"
        );
        // engine dropped = simulated crash
    }

    // Phase 2: recover from same WAL
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert_eq!(
            engine.get_object_state(object_id),
            Some(ObjectState::Alive),
            "object must survive WAL recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.1: Object birth + link must both survive recovery.
/// Verifies topology is rebuilt correctly from WAL.
#[test]
fn object_link_survives_recovery() {
    let wal_path = format!("target/test_obj_link_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj_a: u64 = 100;
    let obj_b: u64 = 200;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.object_link(&mut tx, obj_a, obj_b, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
        assert!(engine.has_link(obj_a, obj_b), "link should exist before crash");
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert_eq!(
            engine.get_object_state(obj_a),
            Some(ObjectState::Alive),
            "obj_a must survive recovery"
        );
        assert_eq!(
            engine.get_object_state(obj_b),
            Some(ObjectState::Alive),
            "obj_b must survive recovery"
        );
        assert!(
            engine.has_link(obj_a, obj_b),
            "link must survive WAL recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.1: Aborted object must NOT appear after recovery.
#[test]
fn aborted_object_not_recovered() {
    let wal_path = format!("target/test_abort_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let object_id: u64 = 999;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, object_id).unwrap();
        engine.abort(&mut tx, veritas_kernel::types::AbortReason::WriteConflict);
        // not committed — should not persist
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert_eq!(
            engine.get_object_state(object_id),
            None,
            "aborted object must not appear after recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
