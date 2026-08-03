use crate::common::new_kernel;
use veritas_kernel::kernel::KernelCall;
use veritas_kernel::types::AbortReason;

#[test]
fn t3_snapshot_isolation_read_own_writes() {
    let tk = new_kernel();
    let state_id = 50;

    let mut tx = tk.kernel.begin_in_object(tk.root_object);
    tk.kernel.write(&mut tx, state_id, vec![1]).unwrap();
    tk.kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    // Write in tx and read back before commit
    let mut tx = tk.kernel.begin_in_object(tk.root_object);
    tk.kernel.write(&mut tx, state_id, vec![9]).unwrap();
    let val = tk.kernel.read(&mut tx, state_id).unwrap();
    assert_eq!(val, vec![9], "Must read own writes");
    tk.kernel.handle(&mut tx, KernelCall::Abort { reason: AbortReason::AlreadyAborted }).unwrap();

    // After abort, must see old value
    let mut read_tx = tk.kernel.begin_in_object(tk.root_object);
    let value = tk.kernel.read(&mut read_tx, state_id).unwrap();
    assert_eq!(value, vec![1], "After abort, must see committed snapshot");
}

#[test]
fn t4_abort_rollback_all() {
    let tk = new_kernel();
    let state_id = 77;

    // Setup committed value
    let mut setup_tx = tk.kernel.begin_in_object(tk.root_object);
    tk.kernel.write(&mut setup_tx, state_id, vec![0]).unwrap();
    tk.kernel.handle(&mut setup_tx, KernelCall::Commit).unwrap();

    // Write then abort
    let mut tx = tk.kernel.begin_in_object(tk.root_object);
    tk.kernel.write(&mut tx, state_id, vec![100]).unwrap();
    tk.kernel.handle(&mut tx, KernelCall::Abort { reason: AbortReason::WriteConflict }).unwrap();

    // Must see original value
    let mut read_tx = tk.kernel.begin_in_object(tk.root_object);
    let val = tk.kernel.read(&mut read_tx, state_id).unwrap();
    assert_eq!(val, vec![0], "Abort must rollback all writes");
}
