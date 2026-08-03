use crate::common::new_kernel;
use veritas_kernel::kernel::{KernelCall, TrapResult};
use veritas_kernel::types::ObjectType;

#[test]
fn o1_object_birth_creates_isolated_entity() {
    let tk = new_kernel();
    let root = tk.root_object;

    let mut tx = tk.kernel.begin_in_object(root);
    let child = match tk.kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).unwrap() {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    tk.kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    assert_ne!(child, root, "child must have unique ObjectId");
    assert_eq!(
        tk.kernel.get_object_state(child),
        Some(veritas_kernel::types::ObjectState::Alive)
    );
}
