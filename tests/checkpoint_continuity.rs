//! Checkpoint 连续性：restore 后世界状态、capability 身份、对象死亡与 state entry 版本保持连续一致。
//!
//! 验证内容：
//! - checkpoint restore 后世界拓扑与 capability 可继续使用。
//! - capability 身份（id）在 restore 后不变。
//! - 对象死亡后不会出现 ghost state。
//! - state entry 版本在 checkpoint 后保持。
//!
//! 对应 VERIFICATION_MAP：checkpoint_continuity.rs
//!
//! 若失败，意味着 checkpoint 路径破坏了状态连续性或身份稳定性不变量。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::*;

fn birth(kernel: &Kernel, ctx: &mut TransactionContext) -> ObjectId {
    match kernel
        .handle(
            ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    }
}
fn grant(
    kernel: &Kernel,
    ctx: &mut TransactionContext,
    grantee: ObjectId,
    resource: ObjectId,
    cap_type: &str,
) -> CapabilityId {
    match kernel
        .handle(
            ctx,
            KernelCall::CapabilityGrant {
                grantor: grantee,
                grantee,
                capability_type: cap_type.to_string(),
                resource,
            },
        )
        .unwrap()
    {
        TrapResult::CapabilityId(id) => id,
        _ => panic!("expected CapabilityId"),
    }
}

/// Checkpoint restore 后世界状态连续，可继续执行并保持拓扑一致。
/// 失败意味着 restore 丢失了 live 状态或破坏了连续性不变量。
#[test]
fn checkpoint_restore_world_continuity() {
    let k_cont = Kernel::new();
    let mut ctx = k_cont.test_begin();
    let o1 = birth(&k_cont, &mut ctx);
    let cap1 = grant(&k_cont, &mut ctx, o1, o1, "read");
    k_cont.handle(&mut ctx, KernelCall::Commit).unwrap();
    let mut ctxw = k_cont.test_begin_in_object(o1);
    k_cont.test_write(&mut ctxw, 1, b"hello".to_vec()).unwrap();
    k_cont.handle(&mut ctxw, KernelCall::Commit).unwrap();
    let root1 = k_cont.state_root();
    let meta = k_cont.create_checkpoint();
    let (gv1, oid1, gs1) = (
        meta.global_version,
        meta.object_id_counter,
        meta.grant_sequence,
    );

    let mut ctx2 = k_cont.test_begin();
    let o2 = birth(&k_cont, &mut ctx2);
    let cap2 = grant(&k_cont, &mut ctx2, o2, o2, "write");
    k_cont.handle(&mut ctx2, KernelCall::Commit).unwrap();
    let mut ctxw2 = k_cont.test_begin_in_object(o2);
    k_cont.test_write(&mut ctxw2, 2, b"world".to_vec()).unwrap();
    k_cont.handle(&mut ctxw2, KernelCall::Commit).unwrap();
    let root_final = k_cont.state_root();
    let final_cont = k_cont.create_checkpoint();

    let k_rest = Kernel::new();
    let mut ctx = k_rest.test_begin();
    let o1r = birth(&k_rest, &mut ctx);
    let cap1r = grant(&k_rest, &mut ctx, o1r, o1r, "read");
    k_rest.handle(&mut ctx, KernelCall::Commit).unwrap();
    let mut ctxw = k_rest.test_begin_in_object(o1r);
    k_rest.test_write(&mut ctxw, 1, b"hello".to_vec()).unwrap();
    k_rest.handle(&mut ctxw, KernelCall::Commit).unwrap();
    assert_eq!(o1r, o1);
    assert_eq!(cap1r, cap1);
    assert_eq!(k_rest.state_root(), root1);
    let snap = k_rest.create_checkpoint();
    assert_eq!(snap.global_version, gv1);
    assert_eq!(snap.object_id_counter, oid1);
    assert_eq!(snap.grant_sequence, gs1);
    k_rest.restore_checkpoint(&snap);

    let mut ctx2 = k_rest.test_begin();
    let o2r = birth(&k_rest, &mut ctx2);
    let cap2r = grant(&k_rest, &mut ctx2, o2r, o2r, "write");
    k_rest.handle(&mut ctx2, KernelCall::Commit).unwrap();
    let mut ctxw2 = k_rest.test_begin_in_object(o2r);
    k_rest.test_write(&mut ctxw2, 2, b"world".to_vec()).unwrap();
    k_rest.handle(&mut ctxw2, KernelCall::Commit).unwrap();
    assert_eq!(o2r, o2);
    assert_eq!(cap2r, cap2);
    assert_eq!(k_rest.state_root(), root_final);
    let final_rest = k_rest.create_checkpoint();
    assert_eq!(final_rest.global_version, final_cont.global_version);
    assert_eq!(final_rest.object_id_counter, final_cont.object_id_counter);
    assert_eq!(final_rest.grant_sequence, final_cont.grant_sequence);
}

/// Capability 身份（capability_id）在 checkpoint restore 后保持不变。
/// 失败意味着身份在持久化/恢复路径上被重新分配，破坏可归因性。
#[test]
fn capability_identity_survives_checkpoint_restore() {
    let kernel = Kernel::new();
    let mut ctx = kernel.test_begin();
    let o1 = birth(&kernel, &mut ctx);
    let o2 = birth(&kernel, &mut ctx);
    let cap_a = grant(&kernel, &mut ctx, o1, o2, "read");
    let cap_b = grant(&kernel, &mut ctx, o1, o2, "read");
    assert_ne!(cap_a, cap_b);
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    let snap = kernel.create_checkpoint();
    let ids_before: Vec<_> = snap
        .capability_records
        .iter()
        .map(|r| r.capability_id)
        .collect();
    assert!(ids_before.contains(&cap_a) && ids_before.contains(&cap_b));
    kernel.restore_checkpoint(&snap);
    let snap2 = kernel.create_checkpoint();
    let ids_after: Vec<_> = snap2
        .capability_records
        .iter()
        .map(|r| r.capability_id)
        .collect();
    assert_eq!(ids_before, ids_after);
    assert!(kernel.holds_capability(cap_a, o1));
    assert!(kernel.holds_capability(cap_b, o1));
}

/// 对象死亡后经 checkpoint restore 不应留下 ghost state。
/// 失败意味着死亡状态未正确持久化，破坏生命周期不变量。
#[test]
fn object_death_no_ghost_state_after_checkpoint() {
    let kernel = Kernel::new();
    let mut ctx = kernel.test_begin();
    let oid = birth(&kernel, &mut ctx);
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    let mut ctx2 = kernel.test_begin_in_object(oid);
    kernel
        .test_write(&mut ctx2, 7, b"ghost-bait".to_vec())
        .unwrap();
    kernel.handle(&mut ctx2, KernelCall::Commit).unwrap();
    let root_alive = kernel.state_root();
    let mut ctx3 = kernel.test_begin_in_object(oid);
    kernel
        .handle(&mut ctx3, KernelCall::ObjectDeath { object_id: oid })
        .unwrap();
    kernel.handle(&mut ctx3, KernelCall::Commit).unwrap();
    let snap_dead = kernel.create_checkpoint();
    assert!(!snap_dead
        .state_entries
        .iter()
        .any(|(a, _)| a.object_id == oid));
    assert!(snap_dead
        .objects
        .iter()
        .any(|o| o.id == oid && o.lifecycle_state == ObjectState::Dead));
    let root_dead = kernel.state_root();
    assert_ne!(root_alive, root_dead);
    kernel.restore_checkpoint(&snap_dead);
    assert!(!kernel
        .create_checkpoint()
        .state_entries
        .iter()
        .any(|(a, _)| a.object_id == oid));
    assert_eq!(kernel.state_root(), root_dead);
}

/// Checkpoint 保留 state entry 的版本号，restore 后版本连续。
/// 失败意味着版本信息丢失，破坏乐观并发/一致性不变量。
#[test]
fn checkpoint_preserves_state_entry_versions() {
    let kernel = Kernel::new();
    let mut ctx = kernel.test_begin();
    let oid = birth(&kernel, &mut ctx);
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    let mut ctx2 = kernel.test_begin_in_object(oid);
    kernel.test_write(&mut ctx2, 1, b"v1".to_vec()).unwrap();
    kernel.handle(&mut ctx2, KernelCall::Commit).unwrap();
    let snap = kernel.create_checkpoint();
    let v_before: Vec<_> = snap
        .state_entries
        .iter()
        .map(|(a, e)| (*a, e.version))
        .collect();
    kernel.restore_checkpoint(&snap);
    let v_after: Vec<_> = kernel
        .create_checkpoint()
        .state_entries
        .iter()
        .map(|(a, e)| (*a, e.version))
        .collect();
    assert_eq!(v_before, v_after);
}
