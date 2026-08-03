use veritas_kernel::engine::VeritasEngine;

/// P29.5: Write a valid WAL, then truncate the last N bytes.
/// Recovery must either succeed with the state before the truncated
/// entry, or return a clean error — never panic, never corrupt.
fn test_truncated_wal(truncate_bytes: usize) {
    let wal_path = format!(
        "target/test_trunc_{}_{}.wal",
        std::process::id(),
        truncate_bytes
    );
    let _ = std::fs::remove_file(&wal_path);

    let obj_id: u64 = 42;

    // Phase 1: create a valid WAL with some operations
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_id).unwrap();
        engine.commit(&mut tx).unwrap();
        // engine dropped → WAL flushed
    }

    // Phase 2: truncate the WAL file
    {
        let mut bytes = std::fs::read(&wal_path).unwrap();
        if bytes.len() > truncate_bytes {
            bytes.truncate(bytes.len() - truncate_bytes);
            std::fs::write(&wal_path, &bytes).unwrap();
        }
    }

    // Phase 3: attempt recovery — must not panic
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        // If the WAL was truncated after Commit, object may or may not exist.
        // Either outcome is acceptable; the invariant is: no panic.
        let _state = engine.get_object_state(obj_id);
        // If we got here without panicking, the test passes.
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.5: Corrupted WAL record — garbage bytes in the middle.
fn test_corrupted_wal(corrupt_offset: usize, corrupt_byte: u8) {
    let wal_path = format!(
        "target/test_corrupt_{}_{}.wal",
        std::process::id(),
        corrupt_offset
    );
    let _ = std::fs::remove_file(&wal_path);

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, 1).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Corrupt a byte
    {
        let mut bytes = std::fs::read(&wal_path).unwrap();
        if corrupt_offset < bytes.len() {
            bytes[corrupt_offset] = corrupt_byte;
            std::fs::write(&wal_path, &bytes).unwrap();
        }
    }

    // Recovery must not panic
    {
        let _engine = VeritasEngine::with_wal_path(wal_path.clone());
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.5: Idempotent recovery — recovering twice from the same WAL
/// must produce the same state.
#[test]
fn recovery_is_idempotent() {
    let wal_path = format!("target/test_idempotent_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj_a: u64 = 10;
    let obj_b: u64 = 20;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Recover once
    let state_after_first;
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        state_after_first = (
            engine.get_object_state(obj_a),
            engine.get_object_state(obj_b),
        );
    }

    // Recover again (idempotent)
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let state_after_second = (
            engine.get_object_state(obj_a),
            engine.get_object_state(obj_b),
        );
        assert_eq!(
            state_after_second, state_after_first,
            "Recovery must be idempotent: second recovery yields same state"
        );
    }

    // Recover a third time
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let state_after_third = (
            engine.get_object_state(obj_a),
            engine.get_object_state(obj_b),
        );
        assert_eq!(
            state_after_third, state_after_first,
            "Recovery must be idempotent across multiple calls"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.5: Empty WAL recovery must succeed (no objects).
#[test]
fn empty_wal_recovery_succeeds() {
    let wal_path = format!("target/test_empty_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // Create empty file
    std::fs::write(&wal_path, b"").unwrap();

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let ids = engine.list_object_ids();
        assert!(ids.is_empty(), "empty WAL recovery should yield no objects");
    }

    let _ = std::fs::remove_file(&wal_path);
}

// ===== Tests =====

#[test]
fn truncated_wal_last_10_bytes() {
    test_truncated_wal(10);
}

#[test]
fn truncated_wal_last_50_bytes() {
    test_truncated_wal(50);
}

#[test]
fn truncated_wal_last_200_bytes() {
    test_truncated_wal(200);
}

#[test]
fn corrupted_wal_middle_byte() {
    test_corrupted_wal(20, 0xFF);
}

#[test]
fn corrupted_wal_early_byte() {
    test_corrupted_wal(5, 0x00);
}
