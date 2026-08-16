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
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn empty_world_root_hash_is_deterministic() {
    let tk1 = common::new_kernel();
    let tk2 = common::new_kernel();
    let h1 = tk1.kernel.test_engine().root_hash();
    let h2 = tk2.kernel.test_engine().root_hash();
    assert_eq!(h1, h2);
    assert_ne!(h1, [0u8; 32]);
}

/// Write 操作改变 root hash。
/// 失败意味着状态变更未反映到状态根。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
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
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
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
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn root_hash_changes_on_link() {
    let tk = common::new_kernel();
    let root = tk.root_object;

    // 创建子对象（root 名下，使 root 持有 child 的 AdminCap）
    let mut tx = tk.kernel.test_begin_in_object(root);
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
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
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
/// Independent golden vector: reconstruct the canonical encoding of the
/// world produced by `common::new_kernel()` and assert that
/// `root_hash()` equals `sha256(that buffer)`.
///
/// This does **not** call any engine hashing helper. The expected digest is
/// built from the documented five-component encoding only.
///
/// World produced by `new_kernel()`:
///   1. ObjectBirth → ObjectId 1 (StateObject, Alive) + self AdminCap
///   2. Write state_id=0 with value = 0u64 LE bytes under object 1
///      → StateStore entry version = 2 (second commit)
///   Topology empty, ScopeRegistry empty.
///
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn root_hash_matches_independent_sha256_golden_vector() {
    let tk = common::new_kernel();
    let root = tk.root_object;
    let actual = tk.kernel.test_engine().root_hash();

    let mut buf: Vec<u8> = Vec::new();

    {
        let object_id: u64 = root;
        let state_id: u64 = 0;
        let value: [u8; 8] = 0u64.to_le_bytes();
        let version: u64 = 2;
        buf.extend_from_slice(&object_id.to_le_bytes());
        buf.extend_from_slice(&state_id.to_le_bytes());
        buf.extend_from_slice(&(value.len() as u64).to_le_bytes());
        buf.extend_from_slice(&value);
        buf.extend_from_slice(&version.to_le_bytes());
    }

    {
        let id: u64 = root;
        let state: u8 = 0;
        let object_type: u8 = 0;
        buf.extend_from_slice(&id.to_le_bytes());
        buf.push(state);
        buf.push(object_type);
    }

    {
        let granted_by: u64 = root;
        let root_holder: u64 = root;
        let resource: u64 = root;
        let capability_type = b"AdminCap";
        buf.extend_from_slice(&granted_by.to_le_bytes());
        buf.extend_from_slice(&root_holder.to_le_bytes());
        buf.extend_from_slice(&resource.to_le_bytes());
        buf.extend_from_slice(&(capability_type.len() as u64).to_le_bytes());
        buf.extend_from_slice(capability_type);
    }

    let expected = veritas_kernel::crypto::sha256(&buf);
    assert_eq!(
        actual, expected,
        "root_hash() must equal independent SHA-256 of the documented canonical encoding.\n\
         root_object={root}\n\
         actual  ={actual:02x?}\n\
         expected={expected:02x?}\n\
         canonical_len={}",
        buf.len()
    );
}

/// Adversarial: 可变长字段的拼接歧义必须被长度前缀消除。
///
/// ["a", "bc"] 与 ["ab", "c"] 必须产生不同的 commitment。
///
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn root_hash_length_prefix_prevents_concatenation_ambiguity() {
    let tk1 = common::new_kernel();
    let root1 = tk1.root_object;

    let mut tx1 = tk1.kernel.test_begin_in_object(root1);
    tk1.kernel.test_write(&mut tx1, 1, b"a".to_vec()).unwrap();
    tk1.kernel.test_write(&mut tx1, 2, b"bc".to_vec()).unwrap();
    tk1.kernel.test_commit(&mut tx1).unwrap();

    let h1 = tk1.kernel.test_engine().root_hash();

    let tk2 = common::new_kernel();
    let root2 = tk2.root_object;

    let mut tx2 = tk2.kernel.test_begin_in_object(root2);
    tk2.kernel.test_write(&mut tx2, 1, b"ab".to_vec()).unwrap();
    tk2.kernel.test_write(&mut tx2, 2, b"c".to_vec()).unwrap();
    tk2.kernel.test_commit(&mut tx2).unwrap();

    let h2 = tk2.kernel.test_engine().root_hash();

    assert_ne!(
        h1, h2,
        "length prefix must prevent concatenation ambiguity: ['a','bc'] vs ['ab','c']"
    );
}
