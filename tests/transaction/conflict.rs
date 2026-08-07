use crate::common::new_kernel;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::kernel::KernelCall;
use veritas_kernel::types::AbortReason;

#[test]
fn t2_conflict_detection() {
    let tk = new_kernel();
    let state_id = 99;
    let initial_data = vec![10];

    // Setup: commit initial state
    let mut tx_setup = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel.test_write(&mut tx_setup, state_id, initial_data).unwrap();
    tk.kernel.handle(&mut tx_setup, KernelCall::Commit).unwrap();

    // Tx1 reads
    let mut tx1 = tk.kernel.test_begin_in_object(tk.root_object);
    let _ = tk.kernel.test_read(&mut tx1, state_id);

    // Tx2 writes and commits
    let mut tx2 = tk.kernel.test_begin_in_object(tk.root_object);
    let tx2_data = vec![99];
    tk.kernel.test_write(&mut tx2, state_id, tx2_data).unwrap();
    tk.kernel.handle(&mut tx2, KernelCall::Commit).expect("Tx2 should commit successfully");

    // Tx1 tries to write and commit — must conflict
    let tx1_data = vec![1];
    tk.kernel.test_write(&mut tx1, state_id, tx1_data).unwrap();
    let res = tk.kernel.handle(&mut tx1, KernelCall::Commit);
    assert!(res.is_err(), "Tx1 must detect write-write conflict");
}
