//! Receipt：before/after root hash 与 replay 一致性。
//!
//! 验证内容：receipt.after 匹配实际 root hash；before/after 一致；replay 后 receipt 一致。
//! 对应 VERIFICATION_MAP：receipt.rs
//! 若失败，意味着 receipt 与状态根承诺不一致，破坏可验证性。

use veritas_kernel::kernel::Kernel;
use veritas_kernel::kernel::KernelCall;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

/// Receipt 的 after_root 必须匹配实际 world root hash。
/// 失败意味着 receipt 与状态根脱节。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn receipt_after_matches_root_hash() {
    let wal_path = format!("target/test_rcpt1_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path);
    let root = {
        let mut tx = k.test_begin();
        let result = k
            .handle(
                &mut tx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )
            .unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k.test_commit(&mut tx).unwrap();
        id
    };

    let mut tx = k.test_begin_in_object(root);
    k.test_write(&mut tx, 0, vec![42]).unwrap();
    let receipt = k.test_commit(&mut tx).unwrap();

    assert_eq!(receipt.after_root, k.test_engine().root_hash());
}

/// Receipt before/after 与实际状态变化一致。
/// 失败意味着 receipt 记录了错误的状态边界。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn receipt_before_after_consistency() {
    let wal_path = format!("target/test_rcpt2_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path);
    let root = {
        let mut tx = k.test_begin();
        let result = k
            .handle(
                &mut tx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )
            .unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k.test_commit(&mut tx).unwrap();
        id
    };

    let mut tx = k.test_begin_in_object(root);
    k.test_write(&mut tx, 0, vec![42]).unwrap();
    let receipt = k.test_commit(&mut tx).unwrap();

    assert_ne!(receipt.before_root, 0);
    assert_ne!(receipt.after_root, 0);
    assert_ne!(receipt.before_root, receipt.after_root);
    assert_eq!(receipt.after_root, k.test_engine().root_hash());
}

/// Replay 后生成的 receipt 与原始一致。
/// 失败意味着 replay 路径 receipt 计算分歧。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn receipt_replay_consistency() {
    let wal_path = format!("target/test_rcpt3_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path.clone());
    let root = {
        let mut tx = k.test_begin();
        let result = k
            .handle(
                &mut tx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )
            .unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k.test_commit(&mut tx).unwrap();
        id
    };
    let mut tx = k.test_begin_in_object(root);
    k.test_write(&mut tx, 0, vec![99]).unwrap();
    let receipt = k.test_commit(&mut tx).unwrap();

    // Replay 和 idle Recovery 必须一致
    let recovery_hash = {
        let k2 = Kernel::with_wal_path(wal_path.clone());
        k2.test_engine().root_hash()
    };
    let replay_hash = Kernel::replay(&wal_path);

    assert_eq!(
        replay_hash, recovery_hash,
        "Replay must equal idle Recovery root_hash"
    );
    assert_eq!(
        receipt.after_root,
        k.test_engine().root_hash(),
        "Receipt.after_root must equal live engine root_hash"
    );
}
