use crate::common::{new_kernel, root_object_id};

/// O3 Invariant: Freeze 状态是只读强约束，被冻结的 Object 严禁任何形式的写入
#[test]
fn o3_frozen_object_rejects_writes() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target_obj = root ^ 0x112233;
    let state_id = 1;

    // 1. Birth 并初始化数据
    let mut tx_setup = kernel.begin();
    kernel.engine.object_birth(&mut tx_setup, target_obj).unwrap();
    kernel.engine.commit(&mut tx_setup).unwrap();

    let mut tx_write = kernel.engine.begin_in_object(target_obj);
    kernel.engine.write(&mut tx_write, state_id, vec![100]).unwrap();
    kernel.engine.commit(&mut tx_write).unwrap();

    // 2. 冻结 Object
    let mut tx_freeze = kernel.engine.begin_in_object(target_obj);
    kernel.engine.object_freeze(&mut tx_freeze, target_obj).unwrap();
    kernel.engine.commit(&mut tx_freeze).unwrap();

    // 3. 尝试在已冻结 Object 上再次写入
    let mut tx_mutate = kernel.engine.begin_in_object(target_obj);
    let write_res = kernel.engine.write(&mut tx_mutate, state_id, vec![200]);

    // 如果 write 本身没拦截，commit 阶段也必须拦截
    let final_res = match write_res {
        Ok(_) => kernel.engine.commit(&mut tx_mutate),
        Err(e) => Err(e),
    };

    // 4. 断言：宪法拦截生效
    assert!(
        final_res.is_err(),
        "O3 Invariant Violation: Mutation allowed on a frozen object!"
    );
}


/// O4.1 Invariant: Death 显式生效，状态跃迁为 ObjectState::Dead
#[test]
fn o4_1_death_state_transition() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target_obj = root ^ 0x445566;

    let mut tx_birth = kernel.begin();
    kernel.engine.object_birth(&mut tx_birth, target_obj).unwrap();
    kernel.engine.commit(&mut tx_birth).unwrap();

    let mut tx_death = kernel.begin();
    kernel.engine.object_death(&mut tx_death, target_obj).unwrap();
    kernel.engine.commit(&mut tx_death).unwrap();

    assert_eq!(
        kernel.engine.get_object_state(target_obj),
        Some(veritas_kernel::types::ObjectState::Dead)
    );
}

/// O4.2 Invariant: Dead Handle 不可再用于写操作或状态转换
#[test]
fn o4_2_dead_handle_invalidation() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target_obj = root ^ 0x778899;
    let state_id = 1;

    let mut tx_birth = kernel.begin();
    kernel.engine.object_birth(&mut tx_birth, target_obj).unwrap();
    kernel.engine.commit(&mut tx_birth).unwrap();

    // 死亡前写入成功
    let mut tx_write = kernel.engine.begin_in_object(target_obj);
    assert!(kernel.engine.write(&mut tx_write, state_id, vec![1, 2, 3]).is_ok());
    kernel.engine.commit(&mut tx_write).unwrap();

    // 发生死亡
    let mut tx_death = kernel.begin();
    kernel.engine.object_death(&mut tx_death, target_obj).unwrap();
    kernel.engine.commit(&mut tx_death).unwrap();

    // 死亡后：写入拒绝
    let mut tx_fail_write = kernel.engine.begin_in_object(target_obj);
    assert!(kernel.engine.write(&mut tx_fail_write, state_id, vec![4, 5, 6]).is_err());

    // 死亡后：冻结拒绝
    let mut tx_fail_freeze = kernel.begin();
    assert!(kernel.engine.object_freeze(&mut tx_fail_freeze, target_obj).is_err());
}

/// O4.3 Invariant: Death 终局不可逆（重复 Death 必须拒绝）
#[test]
fn o4_3_death_irreversibility() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target_obj = root ^ 0xaabbcc;

    let mut tx_birth = kernel.begin();
    kernel.engine.object_birth(&mut tx_birth, target_obj).unwrap();
    kernel.engine.commit(&mut tx_birth).unwrap();

    let mut tx_death = kernel.begin();
    kernel.engine.object_death(&mut tx_death, target_obj).unwrap();
    kernel.engine.commit(&mut tx_death).unwrap();

    // 再次 Death 拒绝
    let mut tx_re_death = kernel.begin();
    assert!(kernel.engine.object_death(&mut tx_re_death, target_obj).is_err());
}

/// P8.1: A --OWNS--> B，death(A) 后 B 也必须 Dead，边被清理
#[test]
fn p8_1_owns_cascade_single() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x1001;
    let b = root ^ 0x1002;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::Owns).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, a).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(
        kernel.engine.get_object_state(a),
        Some(veritas_kernel::types::ObjectState::Dead)
    );
    assert_eq!(
        kernel.engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Dead),
        "OWNS cascade: B must die when owner A dies"
    );
}

/// P8.1: A --OWNS--> B --OWNS--> C，death(A) 后整条链 Dead
#[test]
fn p8_1_owns_cascade_chain() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x2001;
    let b = root ^ 0x2002;
    let c = root ^ 0x2003;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.object_birth(&mut tx, c).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::Owns).unwrap();
    kernel.engine.object_link(&mut tx, b, c, veritas_kernel::types::LinkType::Owns).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, a).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(kernel.engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(kernel.engine.get_object_state(b), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        kernel.engine.get_object_state(c),
        Some(veritas_kernel::types::ObjectState::Dead),
        "OWNS cascade must be transitive"
    );
}

/// P8.1 对照：REFERENCES 不级联
#[test]
fn p8_1_references_no_cascade() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x3001;
    let b = root ^ 0x3002;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine
        .object_link(&mut tx, a, b, veritas_kernel::types::LinkType::References)
        .unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, a).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(kernel.engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        kernel.engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Alive),
        "REFERENCES must not cascade death"
    );
}

/// P8.2.0-1: DEPENDS_ON → to 死亡后，dependent 收到 DependencyInvalidated
#[test]
fn p8_2_depends_on_emits_effect() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x4001;
    let b = root ^ 0x4002;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::DependsOn).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(kernel.engine.get_object_state(b), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        kernel.engine.get_object_state(a),
        Some(veritas_kernel::types::ObjectState::Alive),
        "DEPENDS_ON must NOT cascade death"
    );

    let inv = kernel.engine.last_dependency_invalidations();
    assert!(
        inv.iter().any(|(dep, dependency)| *dep == a && *dependency == b),
        "expected DependencyInvalidated(dependent=A, dependency=B), got {:?}",
        inv
    );
}

/// P8.2.0-2: abort 不产生 DependencyInvalidated
#[test]
fn p8_2_abort_emits_no_effect() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x5001;
    let b = root ^ 0x5002;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::DependsOn).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, b).unwrap();
    kernel.engine.abort(&mut tx, veritas_kernel::types::AbortReason::AlreadyAborted);

    assert_eq!(kernel.engine.get_object_state(b), Some(veritas_kernel::types::ObjectState::Alive));

    let inv = kernel.engine.last_dependency_invalidations();
    assert!(inv.is_empty(), "abort must not emit DependencyInvalidated, got {:?}", inv);
}

/// P8.2.0-3: REFERENCES 不产生 DependencyInvalidated（对照）
#[test]
fn p8_2_references_emits_no_effect() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x6001;
    let b = root ^ 0x6002;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::References).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, b).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(kernel.engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Alive));
    assert_eq!(kernel.engine.get_object_state(b), Some(veritas_kernel::types::ObjectState::Dead));

    let inv = kernel.engine.last_dependency_invalidations();
    assert!(
        !inv.iter().any(|(dep, dependency)| *dep == a && *dependency == b),
        "REFERENCES must not emit DependencyInvalidated, got {:?}",
        inv
    );
}

/// P8.3: resource 死亡后，指向它的 Capability 不能再授权写入（lazy invalidation）
#[test]
fn p8_3_dead_resource_capability_rejected() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target = root ^ 0x7001;
    let state_id = 1u64;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, target).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.engine.begin_in_object(target);
    tx.enforce_capability();
    kernel.engine.write(&mut tx, state_id, vec![1, 2, 3]).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, target).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(
        kernel.engine.get_object_state(target),
        Some(veritas_kernel::types::ObjectState::Dead)
    );

    let mut tx = kernel.engine.begin_in_object(target);
    tx.enforce_capability();
    let write_res = kernel.engine.write(&mut tx, state_id, vec![9, 9, 9]);
    let final_res = match write_res {
        Ok(()) => kernel.engine.commit(&mut tx),
        Err(e) => Err(e),
    };

    assert!(
        final_res.is_err(),
        "P8.3: write via capability targeting Dead resource must be rejected, got {:?}",
        final_res
    );
}

/// P8.3 对照：resource 仍 Alive 时，强制 capability 校验下写入仍成功
#[test]
fn p8_3_alive_resource_capability_still_works() {
    let kernel = new_kernel();
    let root = root_object_id();
    let target = root ^ 0x7002;
    let state_id = 1u64;

    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, target).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.engine.begin_in_object(target);
    tx.enforce_capability();
    kernel.engine.write(&mut tx, state_id, vec![4, 5, 6]).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut tx = kernel.engine.begin_in_object(target);
    tx.enforce_capability();
    kernel.engine.write(&mut tx, state_id, vec![7, 8, 9]).unwrap();
    let res = kernel.engine.commit(&mut tx);
    assert!(res.is_ok(), "Alive resource must still accept authorized writes: {:?}", res);
}

/// Step 1b: verify build_delta().deaths contains only the original request,
/// not OWNS-cascaded objects.
#[test]
fn step1b_build_delta_deaths_before_owns_expansion() {
    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x8001;
    let b = root ^ 0x8002;

    // Setup: A --OWNS--> B
    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::Owns).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    // Request death(A)
    let mut tx = kernel.begin();
    kernel.engine.object_death(&mut tx, a).unwrap();

    // Before commit, pending_deaths contains only A
    let requested = tx.pending_deaths.clone();
    assert_eq!(requested, vec![a], "before OWNS expansion: pending_deaths must be [A]");

    // build_delta with the original request
    let delta = kernel.engine.build_delta(&tx, requested.clone(), 1);

    // Core assertion: Delta.deaths == [A], NOT [A, B]
    assert_eq!(
        delta.deaths,
        vec![a],
        "Delta.deaths must contain only the explicitly requested death A"
    );
    assert_eq!(delta.deaths.len(), 1, "Delta.deaths.len() must be 1");

    // Commit — OWNS cascade still happens at commit time
    kernel.engine.commit(&mut tx).unwrap();

    assert_eq!(kernel.engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        kernel.engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Dead),
        "B must still die via OWNS cascade"
    );
}

/// Step 2a: apply() 必须从 Delta.deaths 出发重新计算 OWNS 闭包，
/// 不依赖 ctx.pending_links。
#[test]
fn step2a_apply_recomputes_owns_closure() {
    use veritas_kernel::types::{LinkType, TransactionDelta};

    let kernel = new_kernel();
    let root = root_object_id();
    let a = root ^ 0x9001;
    let b = root ^ 0x9002;

    // Setup: birth A and B, create A --OWNS--> B, commit
    let mut tx = kernel.begin();
    kernel.engine.object_birth(&mut tx, a).unwrap();
    kernel.engine.object_birth(&mut tx, b).unwrap();
    kernel.engine.object_link(&mut tx, a, b, LinkType::Owns).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    // Build a Delta by hand: only death(A), no OWNS cascade precomputed
    let delta = TransactionDelta {
        tx_id: 1,
        commit_version: 2,
        writes: vec![],
        scope_changes: vec![],
        births: vec![],
        deaths: vec![a],   // only A requested
        freezes: vec![],
        links: vec![],
        unlinks: vec![],
        capability_grants: vec![],
        effects: vec![],
    };

    // Call apply() directly — it must recompute the OWNS closure
    kernel.engine.apply(&delta);

    // Both A and B must be Dead
    assert_eq!(
        kernel.engine.get_object_state(a),
        Some(veritas_kernel::types::ObjectState::Dead)
    );
    assert_eq!(
        kernel.engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Dead),
        "apply() must recompute OWNS closure: B must die even though only A was in delta.deaths"
    );
}

/// Step 2c: 单事务内 A OWNS B，death(A)，恢复后 B 也必须死
#[test]
fn step2c_recovery_single_tx_owns_cascade() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let wal_path = tmp.path().to_str().unwrap().to_string();
    let root = root_object_id();
    let a = root ^ 0xa001;
    let b = root ^ 0xa002;

    // Tx1: birth A, B
    {
        let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin_in_object(root);
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.commit(&mut tx).unwrap();
    }
    // Tx2: link A OWNS B + death(A)
    {
        let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin_in_object(root);
        engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::Owns).unwrap();
        engine.object_death(&mut tx, a).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // 恢复
    let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());

    assert_eq!(engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Dead),
        "Step 2c recovery: B must die via OWNS cascade recomputed by apply()"
    );
}

/// Step 2c: 跨事务级联 — tx1 建 A OWNS B，tx2 death(A)，恢复后 B 必须死
#[test]
fn step2c_recovery_cross_tx_owns_cascade() {
    use tempfile::NamedTempFile;

    let tmp = NamedTempFile::new().unwrap();
    let wal_path = tmp.path().to_str().unwrap().to_string();
    let root = root_object_id();
    let a = root ^ 0xa003;
    let b = root ^ 0xa004;

    // Tx1: birth A, B + link A OWNS B
    {
        let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin_in_object(root);
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.object_link(&mut tx, a, b, veritas_kernel::types::LinkType::Owns).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // Tx2: death(A)
    {
        let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin_in_object(root);
        engine.object_death(&mut tx, a).unwrap();
        engine.commit(&mut tx).unwrap();
    }

    // 恢复
    let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());

    assert_eq!(engine.get_object_state(a), Some(veritas_kernel::types::ObjectState::Dead));
    assert_eq!(
        engine.get_object_state(b),
        Some(veritas_kernel::types::ObjectState::Dead),
        "Step 2c cross-tx: tx2 death(A) + recovery must cascade to B via tx1's OWNS edge"
    );
}

/// Step 3: CRC 损坏的 TransactionCommitted 记录恢复后应被丢弃
#[test]
fn step2c_recovery_orphan_tx_discarded() {
    use tempfile::NamedTempFile;
    use std::io::Write;

    let tmp = NamedTempFile::new().unwrap();
    let wal_path = tmp.path().to_str().unwrap().to_string();
    let root = root_object_id();
    let a = root ^ 0xa005;

    // 写一条 CRC 错误的 TransactionCommitted 记录
    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)
            .unwrap();
        // CRC will not match the payload
        writeln!(file, "LEN=50 CRC=00000000 TXCOMMIT TX=42 VERSION=1 BIRTH {} END", a).unwrap();
    }

    // 恢复
    let engine = veritas_kernel::engine::VeritasEngine::with_wal_path(wal_path.clone());

    // A 不应该存在——CRC 校验失败导致记录被丢弃
    assert_eq!(
        engine.get_object_state(a),
        None,
        "Corrupted TransactionCommitted must be discarded by CRC check"
    );
}

