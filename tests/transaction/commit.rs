use crate::common::new_kernel;

/// T1 Invariant: Commit 必须原子且持久地将 WriteSet 刷入内核物理状态库
#[test]
fn t1_commit_persists_state() {
    let kernel = new_kernel();
    let state_id = 42;
    let payload = vec![1, 2, 3, 4];

    let mut tx = kernel.begin();
    kernel.engine.write(&mut tx, state_id, payload.clone()).unwrap();
    kernel.engine.commit(&mut tx).unwrap();

    let mut read_tx = kernel.begin();
    let val = kernel.engine.read(&mut read_tx, state_id).unwrap();
    assert_eq!(val, payload, "T1 Invariant Violation: Committed data mismatch!");
}
