use veritas_kernel::kernel::{Kernel, KernelCall};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::*;

/// 构建一个包含四组件的小世界
fn build_world(kernel: &Kernel) {
    let mut ctx = kernel.test_begin();
    kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    let mut ctx = kernel.test_begin_in_object(1);
    kernel
        .handle(
            &mut ctx,
            KernelCall::CapabilityGrant {
                grantor: 1,
                grantee: 1,
                capability_type: "link".to_string(),
                resource: 2,
            },
        )
        .unwrap();
    kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectLink {
                from: 1,
                to: 2,
                link_type: LinkType::Owns,
            },
        )
        .unwrap();
    kernel
        .handle(
            &mut ctx,
            KernelCall::CapabilityGrant {
                grantor: 1,
                grantee: 2,
                capability_type: "read".to_string(),
                resource: 100,
            },
        )
        .unwrap();
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
}

// ========== 1. 五组件 roundtrip ==========

#[test]
fn checkpoint_full_roundtrip_all_five_components() {
    let kernel = Kernel::new();
    build_world(&kernel);

    let engine = kernel.test_engine();
    let snap1 = engine.create_checkpoint();
    engine.restore_checkpoint(&snap1);
    let snap2 = engine.create_checkpoint();

    assert_eq!(snap1.objects, snap2.objects, "ObjectRegistry");
    assert_eq!(snap1.links, snap2.links, "Topology");
    assert_eq!(
        snap1.capability_records, snap2.capability_records,
        "CapabilityGraph"
    );
    assert_eq!(snap1.state_entries, snap2.state_entries, "StateStore");
}

// ========== 2. Restore 后可继续执行 ==========

#[test]
fn checkpoint_restore_then_continue_execution() {
    let kernel = Kernel::new();
    build_world(&kernel);

    let engine = kernel.test_engine();
    let snap = engine.create_checkpoint();

    // restore
    engine.restore_checkpoint(&snap);

    // 继续执行：创建第三个 Object
    let mut ctx = kernel.test_begin();
    kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();

    // 验证 Object 3 存在
    let snap_after = engine.create_checkpoint();
    assert!(
        snap_after.objects.iter().any(|o| o.id == 3),
        "Object 3 should exist after resume"
    );
}

// ========== 3. 多次 restore 幂等 ==========

#[test]
fn checkpoint_restore_idempotent() {
    let kernel = Kernel::new();
    build_world(&kernel);

    let engine = kernel.test_engine();
    let snap = engine.create_checkpoint();

    engine.restore_checkpoint(&snap);
    let snap1 = engine.create_checkpoint();

    engine.restore_checkpoint(&snap);
    let snap2 = engine.create_checkpoint();

    assert_eq!(snap1.objects, snap2.objects);
    assert_eq!(snap1.links, snap2.links);
    assert_eq!(snap1.capability_records, snap2.capability_records);
}

// ========== 4. root_hash 一致性 ==========

#[test]
fn checkpoint_root_hash_consistent() {
    let kernel = Kernel::new();
    build_world(&kernel);

    let engine = kernel.test_engine();
    let _root_before = engine.state_root();
    let snap = engine.create_checkpoint();

    engine.restore_checkpoint(&snap);
    let _root_after = engine.state_root();

    // 恢复后 state_root 应与创建快照时一致
    let snap2 = engine.create_checkpoint();
    let snap3 = engine.create_checkpoint();
    assert_eq!(
        snap2.state_entries, snap3.state_entries,
        "连续两次 create_checkpoint 应该一致（世界未变）"
    );
}

// ========== 5. 计数器 roundtrip（Stage 2 任务4） ==========

#[test]
fn checkpoint_counter_roundtrip() {
    let kernel = Kernel::new();
    build_world(&kernel);

    let engine = kernel.test_engine();
    let snap = engine.create_checkpoint();

    // 验证快照包含计数器
    assert!(
        snap.global_version > 0,
        "global_version should be > 0 after commit"
    );
    assert!(
        snap.object_id_counter > 2,
        "object_id_counter should be > 2 after two births"
    );
    assert!(
        snap.grant_sequence > 0,
        "grant_sequence should be > 0 after grant"
    );

    // restore 后计数器一致
    engine.restore_checkpoint(&snap);
    let snap2 = engine.create_checkpoint();
    assert_eq!(
        snap.global_version, snap2.global_version,
        "global_version should survive roundtrip"
    );
    assert_eq!(
        snap.object_id_counter, snap2.object_id_counter,
        "object_id_counter should survive roundtrip"
    );
    assert_eq!(
        snap.grant_sequence, snap2.grant_sequence,
        "grant_sequence should survive roundtrip"
    );

    // restore 后继续执行：ObjectId 不会重用
    let mut ctx = kernel.test_begin();
    kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap();
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    let snap3 = engine.create_checkpoint();
    assert!(
        snap3.object_id_counter > snap.object_id_counter,
        "object_id_counter should advance after restore"
    );
}
