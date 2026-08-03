use crate::common::new_kernel;
use veritas_kernel::kernel::KernelCall;

#[test]
fn t1_commit_persists_state() {
    let tk = new_kernel();
    let state_id = 42;
    let payload = vec![1, 2, 3, 4];

    let mut tx = tk.kernel.begin_in_object(tk.root_object);
    tk.kernel.write(&mut tx, state_id, payload.clone()).unwrap();
    tk.kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    let mut read_tx = tk.kernel.begin_in_object(tk.root_object);
    let val = tk.kernel.read(&mut read_tx, state_id).unwrap();
    assert_eq!(val, payload, "T1 Invariant Violation: Committed data mismatch!");
}
