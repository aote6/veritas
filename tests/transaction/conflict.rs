use crate::common::new_kernel;

/// T4 Invariant (Conflict Detection): 并发冲突下旧版本视图提交必须被 MVCC 拒绝
#[test]
fn t4_optimistic_concurrency_conflict_aborts() {
    let kernel = new_kernel();
    let state_id = 77;
    let initial_data = vec![0u8];
    let tx1_data = vec![1u8];
    let tx2_data = vec![2u8];

    // Setup: 先写入一个基础数据 version 0
    let mut tx_setup = kernel.begin();
    kernel.engine.write(&mut tx_setup, state_id, initial_data).unwrap();
    kernel.engine.commit(&mut tx_setup).unwrap();

    // 1. tx1 开启并读取 state_id (锁定在 version 0)
    let mut tx1 = kernel.begin();
    let _ = kernel.engine.read(&mut tx1, state_id);

    // 2. tx2 后发并先一步修改 state_id 成功提交 (升级到 version 1)
    let mut tx2 = kernel.begin();
    kernel.engine.write(&mut tx2, state_id, tx2_data).unwrap();
    kernel.engine.commit(&mut tx2).expect("Tx2 should commit successfully");

    // 3. tx1 尝试写并提交 (基于过期 version 0)
    kernel.engine.write(&mut tx1, state_id, tx1_data).unwrap();
    let res = kernel.engine.commit(&mut tx1);

    // 4. 不变量断言：tx1 提交必须因 OCC/MVCC 冲突失败
    assert!(
        res.is_err(),
        "T4 Invariant Violation: Stale transaction succeeded commit over concurrent write!"
    );
}
