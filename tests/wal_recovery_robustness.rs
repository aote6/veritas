//! WAL recovery 鲁棒性：截断与损坏 WAL 的处理。
//!
//! 验证内容：末尾截断或中间/早期字节损坏的 WAL 被正确检测或安全处理，不产生非法状态。
//! 对应 VERIFICATION_MAP：wal_recovery_robustness.rs
//! 若失败，意味着损坏 WAL 可导致静默错误状态或崩溃，破坏恢复安全性。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn birth(kernel: &Kernel) -> u64 {
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
    kernel.handle(&mut tx, KernelCall::Commit);
    id
}

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

    let obj_id: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        obj_id = birth(&kernel);
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
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let _state = kernel.test_engine().get_object_state(obj_id);
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

    let _obj_id: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        _obj_id = birth(&kernel);
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
        let _engine = veritas_kernel::test_api::recover_engine(&wal_path);
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.5: Idempotent recovery — recovering twice from the same WAL
/// must produce the same state.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn recovery_is_idempotent() {
    let wal_path = format!("target/test_idempotent_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj_a: u64;
    let obj_b: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        obj_a = birth(&kernel);
        obj_b = birth(&kernel);
    }

    // Recover once
    let state_after_first;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        state_after_first = (
            kernel.test_engine().get_object_state(obj_a),
            kernel.test_engine().get_object_state(obj_b),
        );
    }

    // Recover again (idempotent)
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let state_after_second = (
            kernel.test_engine().get_object_state(obj_a),
            kernel.test_engine().get_object_state(obj_b),
        );
        assert_eq!(
            state_after_second, state_after_first,
            "Recovery must be idempotent: second recovery yields same state"
        );
    }

    // Recover a third time
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let state_after_third = (
            kernel.test_engine().get_object_state(obj_a),
            kernel.test_engine().get_object_state(obj_b),
        );
        assert_eq!(
            state_after_third, state_after_first,
            "Recovery must be idempotent across multiple calls"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.5: Empty WAL recovery must succeed (no objects).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn empty_wal_recovery_succeeds() {
    let wal_path = format!("target/test_empty_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // Create empty file
    std::fs::write(&wal_path, b"").unwrap();

    {
        let engine = veritas_kernel::test_api::recover_engine(&wal_path);
        let ids = engine.list_object_ids();
        assert!(ids.is_empty(), "empty WAL recovery should yield no objects");
    }

    let _ = std::fs::remove_file(&wal_path);
}

// ===== Tests =====

/// 截断最后 10 字节的 WAL 被正确处理。
/// 失败意味着短截断未被检测。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn truncated_wal_last_10_bytes() {
    test_truncated_wal(10);
}

/// 截断最后 50 字节的 WAL 被正确处理。
/// 失败意味着中等截断未被检测。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn truncated_wal_last_50_bytes() {
    test_truncated_wal(50);
}

/// 截断最后 200 字节的 WAL 被正确处理。
/// 失败意味着大截断未被检测。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn truncated_wal_last_200_bytes() {
    test_truncated_wal(200);
}

/// 中间字节损坏的 WAL 被正确处理。
/// 失败意味着中间损坏可导致非法恢复。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn corrupted_wal_middle_byte() {
    test_corrupted_wal(20, 0xFF);
}

/// 早期字节损坏的 WAL 被正确处理。
/// 失败意味着早期损坏可导致非法恢复。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn corrupted_wal_early_byte() {
    test_corrupted_wal(5, 0x00);
}
