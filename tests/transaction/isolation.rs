use crate::common::new_kernel;
use veritas_kernel::types::AbortReason;

/// T2 Invariant: Abort 强保证回滚至事务开启前/持久库中的原始状态，脏写入彻底丢弃
#[test]
fn t2_abort_has_no_effect() {
    let kernel = new_kernel();
    let state_id = 99;

    // 1. 在 root_object 下合规 Commit 初始值 vec![1]
    let mut setup_tx = kernel.begin();
    kernel.engine.write(&mut setup_tx, state_id, vec![1]).unwrap();
    kernel.engine.commit(&mut setup_tx).unwrap();

    // 2. 开启新事务尝试脏写 vec![9]，然后 Abort
    let mut tx = kernel.begin();
    kernel.engine.write(&mut tx, state_id, vec![9]).unwrap();
    kernel.engine.abort(&mut tx, AbortReason::AlreadyAborted);

    // 3. 验证全局状态依旧是 committed 的 vec![1]
    let mut read_tx = kernel.begin();
    let value = kernel.engine.read(&mut read_tx, state_id).unwrap();

    assert_eq!(
        value,
        vec![1],
        "T2 Invariant Violation: Aborted write leaked into global state!"
    );
}

/// T3 Invariant: Transaction 本地读写视图优先级高于底座持久库，保证 Read-Your-Own-Write
#[test]
fn t3_read_your_write() {
    let kernel = new_kernel();
    let state_id = 10;

    // 1. 在 root_object 下合规 Commit 初始值 vec![0]
    let mut setup_tx = kernel.begin();
    kernel.engine.write(&mut setup_tx, state_id, vec![0]).unwrap();
    kernel.engine.commit(&mut setup_tx).unwrap();

    // 2. 本地事务写入 vec![100]，未 Commit 前读取
    let mut tx = kernel.begin();
    kernel.engine.write(&mut tx, state_id, vec![100]).unwrap();

    let value = kernel.engine.read(&mut tx, state_id).unwrap();

    assert_eq!(
        value,
        vec![100],
        "T3 Invariant Violation: Transaction cannot see its own uncommitted write!"
    );
}
