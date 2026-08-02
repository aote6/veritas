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
