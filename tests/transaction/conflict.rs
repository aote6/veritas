use crate::common::new_kernel;
use veritas_kernel::kernel::KernelCall;
use veritas_kernel::test_api::KernelTestExt;

/// @category: B
/// @layer: transaction
/// @testworld: FORBIDDEN
/// @req: TX-04
#[test]
fn t2_conflict_detection() {
    let tk = new_kernel();
    let state_id = 99;
    let initial_data = vec![10];

    // Setup: commit initial state
    let mut tx_setup = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel
        .test_write(&mut tx_setup, state_id, initial_data)
        .unwrap();
    tk.kernel.handle(&mut tx_setup, KernelCall::Commit);

    // Tx1 reads
    let mut tx1 = tk.kernel.test_begin_in_object(tk.root_object);
    let _ = tk.kernel.test_read(&mut tx1, state_id);

    // Tx2 writes and commits
    let mut tx2 = tk.kernel.test_begin_in_object(tk.root_object);
    let tx2_data = vec![99];
    tk.kernel.test_write(&mut tx2, state_id, tx2_data).unwrap();
    tk.kernel
        .handle(&mut tx2, KernelCall::Commit)
        .expect("Tx2 should commit successfully");

    // Tx1 tries to write and commit — must conflict
    let tx1_data = vec![1];
    tk.kernel.test_write(&mut tx1, state_id, tx1_data).unwrap();
    let res = tk.kernel.handle(&mut tx1, KernelCall::Commit);
    assert!(res.is_err(), "Tx1 must detect write-write conflict");
}

/// @category: B
/// @layer: transaction
/// @testworld: FORBIDDEN
/// @req: TX-04
#[test]
fn test_blind_write_write_conflict() {
    let tk = new_kernel();
    let root = tk.root_object;

    let mut tx1 = tk.kernel.test_begin_in_object(root);
    let mut tx2 = tk.kernel.test_begin_in_object(root);

    let target_addr = 88888;

    // 确保 read_set 绝对为空（纯写/盲写）
    tk.kernel
        .test_write(&mut tx1, target_addr, vec![1])
        .unwrap();
    tk.kernel
        .test_write(&mut tx2, target_addr, vec![2])
        .unwrap();

    // T1 先提交，成功将 target_addr 的 entry.version 提升至 commit_version
    let res1 = tk.kernel.test_commit(&mut tx1);
    assert!(res1.is_ok(), "T1 commit should succeed");

    // T2 后提交，在未读取 target_addr 的情况下，预期必须被拦截为 WriteConflict
    let res2 = tk.kernel.test_commit(&mut tx2);
    assert!(
        res2.is_err(),
        "T2 commit MUST fail with WriteConflict on blind write conflict"
    );
}
