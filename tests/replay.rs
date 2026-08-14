//! Replay：空 WAL、与 recovery 等价、确定性、不同 ops 产生不同 hash。
//!
//! 验证内容：replay 空 WAL 返回非零；replay 结果与 recovery idle 等价；确定性；不同操作序列 hash 不同。
//! 对应 VERIFICATION_MAP：replay.rs
//! 若失败，意味着 replay 引擎确定性或与 recovery 路径一致性被破坏。

use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectType;
use veritas_kernel::kernel::KernelCall;

/// 空 WAL replay 返回非零（有意义的结果/状态）。
/// 失败意味着空路径处理错误。
#[test]
fn replay_empty_wal_returns_nonzero() {
    let wal_path = format!("target/test_replay_empty_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // 创建空 WAL
    let k = Kernel::with_wal_path(wal_path.clone());
    let idle_hash = k.test_engine().root_hash();
    drop(k);

    let replay_hash = Kernel::replay(&wal_path);
    assert_ne!(replay_hash, 0);
    assert_eq!(replay_hash, idle_hash,
        "Replay of empty WAL must equal idle Recovery root_hash");
}

/// Replay 结果与 recovery idle 状态等价。
/// 失败意味着两条恢复路径分歧。
#[test]
fn replay_equals_recovery_idle() {
    let wal_path = format!("target/test_replay_idle_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // 写操作到 WAL
    let k1 = Kernel::with_wal_path(wal_path.clone());
    let root = {
        let mut tx = k1.test_begin();
        let result = k1.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k1.test_commit(&mut tx).unwrap();
        id
    };
    let mut tx = k1.test_begin_in_object(root);
    k1.test_write(&mut tx, 0, vec![42]).unwrap();
    k1.test_commit(&mut tx).unwrap();
    drop(k1);

    // Recovery — 不操作
    let k2 = Kernel::with_wal_path(wal_path.clone());
    let recovery_hash = k2.test_engine().root_hash();
    drop(k2);

    // Replay
    let replay_hash = Kernel::replay(&wal_path);

    assert_eq!(replay_hash, recovery_hash);
}

/// 相同输入多次 replay 结果完全一致。
/// 失败意味着确定性执行承诺被破坏。
#[test]
fn replay_is_deterministic() {
    let wal_path = format!("target/test_replay_det_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path.clone());
    let root = {
        let mut tx = k.test_begin();
        let result = k.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k.test_commit(&mut tx).unwrap();
        id
    };
    let mut tx = k.test_begin_in_object(root);
    k.test_write(&mut tx, 0, vec![99]).unwrap();
    k.test_commit(&mut tx).unwrap();
    drop(k);

    let h1 = Kernel::replay(&wal_path);
    let h2 = Kernel::replay(&wal_path);
    assert_eq!(h1, h2);
}

/// 不同操作序列产生不同 root hash。
/// 失败意味着 hash 碰撞或未正确反映状态变化。
#[test]
fn replay_different_ops_different_hash() {
    let wal1 = format!("target/test_replay_diff1_{}.wal", std::process::id());
    let wal2 = format!("target/test_replay_diff2_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal1);
    let _ = std::fs::remove_file(&wal2);

    // WAL1: write [1]
    let k1 = Kernel::with_wal_path(wal1.clone());
    let root1 = {
        let mut tx = k1.test_begin();
        let result = k1.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k1.test_commit(&mut tx).unwrap();
        id
    };
    let mut tx = k1.test_begin_in_object(root1);
    k1.test_write(&mut tx, 0, vec![1]).unwrap();
    k1.test_commit(&mut tx).unwrap();
    drop(k1);

    // WAL2: write [2]
    let k2 = Kernel::with_wal_path(wal2.clone());
    let root2 = {
        let mut tx = k2.test_begin();
        let result = k2.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k2.test_commit(&mut tx).unwrap();
        id
    };
    let mut tx = k2.test_begin_in_object(root2);
    k2.test_write(&mut tx, 0, vec![2]).unwrap();
    k2.test_commit(&mut tx).unwrap();
    drop(k2);

    let h1 = Kernel::replay(&wal1);
    let h2 = Kernel::replay(&wal2);
    assert_ne!(h1, h2);
}
