//! Root Hash：空世界确定性、write/birth/link 改变 hash、顺序无关。
//!
//! 验证内容：空世界 root hash 确定；状态变更改变 hash；同内容不同顺序 hash 相同。
//! 对应 VERIFICATION_MAP：root_hash.rs
//! 若失败，意味着状态根计算不正确或非确定性，破坏可验证承诺。

use veritas_kernel::kernel::{KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{LinkType, ObjectType};

mod common;

/// 空世界 root hash 在多次计算中保持确定。
/// 失败意味着空状态根非确定性。
#[test]
fn empty_world_root_hash_is_deterministic() {
    let tk1 = common::new_kernel();
    let tk2 = common::new_kernel();
    let h1 = tk1.kernel.test_engine().root_hash();
    let h2 = tk2.kernel.test_engine().root_hash();
    assert_eq!(h1, h2);
    assert_ne!(h1, 0);
}

/// Write 操作改变 root hash。
/// 失败意味着状态变更未反映到状态根。
#[test]
fn root_hash_changes_on_write() {
    let tk = common::new_kernel();
    let root = tk.root_object;
    let before = tk.kernel.test_engine().root_hash();

    let mut tx = tk.kernel.test_begin_in_object(root);
    tk.kernel.test_write(&mut tx, 0, vec![1, 2, 3]).unwrap();
    tk.kernel.test_commit(&mut tx).unwrap();

    let after = tk.kernel.test_engine().root_hash();
    assert_ne!(before, after);
}

/// ObjectBirth 改变 root hash。
/// 失败意味着对象创建未反映到状态根。
#[test]
fn root_hash_changes_on_birth() {
    let tk = common::new_kernel();
    let before = tk.kernel.test_engine().root_hash();

    let mut tx = tk.kernel.test_begin();
    let result = tk
        .kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    let _new_id = match result {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    tk.kernel.test_commit(&mut tx).unwrap();

    let after = tk.kernel.test_engine().root_hash();
    assert_ne!(before, after);
}

/// ObjectLink 改变 root hash。
/// 失败意味着拓扑变更未反映到状态根。
#[test]
fn root_hash_changes_on_link() {
    let tk = common::new_kernel();
    let root = tk.root_object;

    // 创建子对象
    let mut tx = tk.kernel.test_begin();
    let result = tk
        .kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    let child = match result {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    tk.kernel.test_commit(&mut tx).unwrap();

    let before = tk.kernel.test_engine().root_hash();

    // 建立 Link
    let mut tx2 = tk.kernel.test_begin_in_object(root);
    tk.kernel
        .handle(
            &mut tx2,
            KernelCall::CapabilityGrant {
                grantor: root,
                grantee: root,
                capability_type: "link".to_string(),
                resource: child,
            },
        )
        .unwrap();
    tk.kernel
        .handle(
            &mut tx2,
            KernelCall::ObjectLink {
                from: root,
                to: child,
                link_type: LinkType::Owns,
            },
        )
        .unwrap();
    tk.kernel.test_commit(&mut tx2).unwrap();

    let after = tk.kernel.test_engine().root_hash();
    assert_ne!(before, after);
}

/// 相同内容不同应用顺序产生相同 root hash。
/// 失败意味着 root hash 对顺序敏感，破坏规范承诺。
#[test]
fn root_hash_order_independent() {
    let tk1 = common::new_kernel();
    let tk2 = common::new_kernel();

    // tk1: write state_id=0 then state_id=1
    {
        let mut tx = tk1.kernel.test_begin_in_object(tk1.root_object);
        tk1.kernel.test_write(&mut tx, 0, vec![1]).unwrap();
        tk1.kernel.test_write(&mut tx, 1, vec![2]).unwrap();
        tk1.kernel.test_commit(&mut tx).unwrap();
    }

    // tk2: write state_id=1 then state_id=0
    {
        let mut tx = tk2.kernel.test_begin_in_object(tk2.root_object);
        tk2.kernel.test_write(&mut tx, 1, vec![2]).unwrap();
        tk2.kernel.test_write(&mut tx, 0, vec![1]).unwrap();
        tk2.kernel.test_commit(&mut tx).unwrap();
    }

    assert_eq!(
        tk1.kernel.test_engine().root_hash(),
        tk2.kernel.test_engine().root_hash()
    );
}
