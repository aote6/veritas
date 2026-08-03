use veritas_kernel::capability::capability_id_of;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::ObjectType;

fn birth(kernel: &Kernel) -> u64 {
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

/// P4.x: Object 创建时授予的 AdminCap 必须在 commit 后正确写入
#[test]
fn capability_grant_visible_after_commit() {
    let wal_path = format!("target/test_cap_visible_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);
    let kernel = Kernel::with_wal_path(wal_path.clone());

    let seq_before = kernel.engine().capability_sequence();
    let target = birth(&kernel);

    let expected_cap_id = capability_id_of(target, target, target, seq_before + 1);
    assert!(
        kernel.engine().holds_capability(expected_cap_id, target),
        "AdminCap should be held by object after commit"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x: AdminCap 必须在 crash + restart 后仍然存在
#[test]
fn capability_survives_recovery() {
    let wal_path = format!("target/test_cap_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let target: u64;
    let expected_cap_id: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let seq_before = kernel.engine().capability_sequence();
        target = birth(&kernel);
        expected_cap_id = capability_id_of(target, target, target, seq_before + 1);

        assert!(
            kernel.engine().holds_capability(expected_cap_id, target),
            "sanity check before restart failed"
        );
    }

    let recovered = Kernel::with_wal_path(wal_path.clone());
    assert!(
        recovered.engine().holds_capability(expected_cap_id, target),
        "AdminCap must survive engine restart via WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x: abort 后 AdminCap 不能残留
#[test]
fn capability_grant_no_leak_on_abort() {
    let wal_path = format!("target/test_cap_no_leak_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);
    let kernel = Kernel::with_wal_path(wal_path.clone());

    let seq_before = kernel.engine().capability_sequence();

    let mut tx = kernel.begin();
    let target = match kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).unwrap() {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Abort {
        reason: veritas_kernel::types::AbortReason::WriteConflict,
    }).unwrap();

    let would_be_cap_id = capability_id_of(target, target, target, seq_before + 1);
    assert!(
        !kernel.engine().holds_capability(would_be_cap_id, target),
        "AdminCap must not leak into capability_graph after abort"
    );

    let _ = std::fs::remove_file(&wal_path);
}
