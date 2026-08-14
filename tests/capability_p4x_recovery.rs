//! P4.x: CapabilityGrant 在 commit / crash-recovery / abort 路径上的可见性与不泄漏不变量。
//!
//! 验证内容：
//! - Object 创建时授予的 AdminCap 在 commit 后正确写入 capability graph。
//! - AdminCap 在 crash + restart（WAL recovery）后仍然存在。
//! - abort 后 AdminCap 不能残留。
//!
//! 对应 VERIFICATION_MAP：capability_p4x_recovery.rs
//!
//! 若失败，意味着 CapabilityGrant 在持久化或恢复路径上丢失/泄漏，破坏能力拓扑与事务原子性不变量。

use veritas_kernel::capability::capability_id_of;
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
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

/// P4.x: Object 创建时授予的 AdminCap 必须在 commit 后正确写入
/// @category: B
/// @layer: transaction
/// @testworld: FORBIDDEN
/// @req: TX-03
#[test]
fn capability_grant_visible_after_commit() {
    let wal_path = format!(
        "{}/test_cap_visible_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal_path);
    let kernel = Kernel::with_wal_path(wal_path.clone());

    let seq_before = kernel.test_engine().capability_sequence();
    let target = birth(&kernel);

    let expected_cap_id = capability_id_of(target, target, target, seq_before + 1);
    assert!(
        kernel
            .test_engine()
            .holds_capability(expected_cap_id, target),
        "AdminCap should be held by object after commit"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x: AdminCap 必须在 crash + restart 后仍然存在
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-03
#[test]
fn capability_survives_recovery() {
    let wal_path = format!(
        "{}/test_cap_recovery_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal_path);

    let target: u64;
    let expected_cap_id: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let seq_before = kernel.test_engine().capability_sequence();
        target = birth(&kernel);
        expected_cap_id = capability_id_of(target, target, target, seq_before + 1);

        assert!(
            kernel
                .test_engine()
                .holds_capability(expected_cap_id, target),
            "sanity check before restart failed"
        );
    }

    let recovered = Kernel::with_wal_path(wal_path.clone());
    assert!(
        recovered
            .test_engine()
            .holds_capability(expected_cap_id, target),
        "AdminCap must survive engine restart via WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x: abort 后 AdminCap 不能残留
/// @category: B
/// @layer: transaction
/// @testworld: FORBIDDEN
/// @req: TX-03
#[test]
fn capability_grant_no_leak_on_abort() {
    let wal_path = format!(
        "{}/test_cap_no_leak_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal_path);
    let kernel = Kernel::with_wal_path(wal_path.clone());

    let seq_before = kernel.test_engine().capability_sequence();

    let mut tx = kernel.test_begin();
    let target = match kernel
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
    kernel
        .handle(
            &mut tx,
            KernelCall::Abort {
                reason: veritas_kernel::types::AbortReason::WriteConflict,
            },
        )
        .unwrap();

    let would_be_cap_id = capability_id_of(target, target, target, seq_before + 1);
    assert!(
        !kernel
            .test_engine()
            .holds_capability(would_be_cap_id, target),
        "AdminCap must not leak into capability_graph after abort"
    );

    let _ = std::fs::remove_file(&wal_path);
}
