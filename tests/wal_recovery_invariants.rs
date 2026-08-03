use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectState, LinkType, AbortReason};

/// P29.2: Birth → Death sequence must be correctly replayed.
/// After recovery, object must be Dead (not Alive, not missing).
#[test]
fn recovery_invariant_birth_then_death() {
    let wal_path = format!("target/test_inv_birth_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj: u64 = 1;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, obj).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert!(engine.is_object_dead(obj), "object must be dead before crash");
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert!(engine.is_object_dead(obj), "object must be dead after recovery");
        // Dead is terminal — not Alive, not Frozen
        assert_ne!(engine.get_object_state(obj), Some(ObjectState::Alive));
        assert_ne!(engine.get_object_state(obj), Some(ObjectState::Frozen));
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Freeze → Death sequence.
/// After recovery, final state must be Dead (not Frozen).
#[test]
fn recovery_invariant_birth_freeze_then_death() {
    let wal_path = format!("target/test_inv_birth_freeze_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj: u64 = 2;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_freeze(&mut tx2, obj).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        engine.object_death(&mut tx3, obj).unwrap();
        engine.commit(&mut tx3).unwrap();

        assert!(engine.is_object_dead(obj), "must be dead before crash");
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert!(engine.is_object_dead(obj), "must be dead after recovery");
        assert_eq!(
            engine.get_object_state(obj),
            Some(ObjectState::Dead),
            "final state must be Dead, not Frozen"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Link → Unlink.
/// After recovery, link must not exist, objects must be alive.
#[test]
fn recovery_invariant_link_then_unlink() {
    let wal_path = format!("target/test_inv_link_unlink_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64 = 10;
    let b: u64 = 20;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.object_link(&mut tx, a, b, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_unlink(&mut tx2, a, b).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert!(!engine.has_link(a, b), "link must be removed before crash");
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert_eq!(engine.get_object_state(a), Some(ObjectState::Alive));
        assert_eq!(engine.get_object_state(b), Some(ObjectState::Alive));
        assert!(!engine.has_link(a, b), "link must not exist after recovery");
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Link → Death of owner.
/// After recovery: owner Dead, link gone, no dangling edges.
#[test]
fn recovery_invariant_owner_death_removes_link() {
    let wal_path = format!("target/test_inv_owner_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64 = 100;
    let owned: u64 = 200;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, owner).unwrap();
        engine.object_birth(&mut tx, owned).unwrap();
        engine.object_link(&mut tx, owner, owned, LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, owner).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert!(engine.is_object_dead(owner), "owner must be dead");
        // OWNS cascade: owned also dies
        assert!(engine.is_object_dead(owned), "owned must cascade to dead");
        assert!(!engine.has_link(owner, owned), "link must be removed");
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert!(engine.is_object_dead(owner), "owner dead after recovery");
        assert!(engine.is_object_dead(owned), "owned dead after recovery");
        assert!(!engine.has_link(owner, owned), "no dangling link after recovery");
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Duplicate birth must not corrupt recovery state.
#[test]
fn recovery_invariant_duplicate_birth_rejected() {
    let wal_path = format!("target/test_inv_dup_birth_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj: u64 = 50;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        // Second birth in same tx should be rejected
        let mut tx2 = engine.begin();
        let result = engine.object_birth(&mut tx2, obj);
        assert!(result.is_err(), "duplicate birth must be rejected");
        engine.abort(&mut tx2, AbortReason::WriteConflict);
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert_eq!(
            engine.get_object_state(obj),
            Some(ObjectState::Alive),
            "object must still be alive after recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Death → Birth (same id) — death is terminal, reborn rejected.
#[test]
fn recovery_invariant_no_rebirth_after_death() {
    let wal_path = format!("target/test_inv_no_rebirth_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj: u64 = 75;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, obj).unwrap();
        engine.commit(&mut tx2).unwrap();

        // Re-birth with same id must fail
        let mut tx3 = engine.begin();
        let result = engine.object_birth(&mut tx3, obj);
        assert!(result.is_err(), "re-birth after death must be rejected");
        engine.abort(&mut tx3, AbortReason::WriteConflict);
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        assert!(engine.is_object_dead(obj), "object must remain dead after recovery");
    }

    let _ = std::fs::remove_file(&wal_path);
}
