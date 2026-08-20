use crate::common::new_kernel;
use veritas_kernel::kernel::{KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: OBJ-01
#[test]
fn memory_isolated_per_object() {
    let tk = new_kernel();
    let root = tk.root_object;

    let mut tx = tk.kernel.test_begin_in_object(root);
    let a = match tk
        .kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    let b = match tk
        .kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    tk.kernel.handle(&mut tx, KernelCall::Commit);

    let mut tx_a = tk.kernel.test_begin_in_object(a);
    tk.kernel.test_write(&mut tx_a, 0, vec![1, 2, 3]).unwrap();
    tk.kernel.handle(&mut tx_a, KernelCall::Commit);

    let mut tx_b = tk.kernel.test_begin_in_object(b);
    tk.kernel.test_write(&mut tx_b, 0, vec![4, 5, 6]).unwrap();
    tk.kernel.handle(&mut tx_b, KernelCall::Commit);

    let mut read_a = tk.kernel.test_begin_in_object(a);
    let val_a = tk.kernel.test_read(&mut read_a, 0).unwrap();
    let mut read_b = tk.kernel.test_begin_in_object(b);
    let val_b = tk.kernel.test_read(&mut read_b, 0).unwrap();

    assert_eq!(val_a, vec![1, 2, 3]);
    assert_eq!(val_b, vec![4, 5, 6]);
}
