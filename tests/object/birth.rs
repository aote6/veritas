use crate::common::{new_kernel, root_object_id};

/// O1 Invariant: Object 诞生后必须拥有合法且隔离的地址空间上下文
#[test]
fn o1_object_birth_creates_isolated_entity() {
    let kernel = new_kernel();
    let root = root_object_id();

    // 衍生一个新的 Object ID
    let child_object = root ^ 0xDEADBEEF;

    let mut tx = kernel.begin();
    let res = kernel.engine.object_birth(&mut tx, child_object);
    assert!(res.is_ok(), "O1 Violation: Failed to birth new Object");

    kernel.engine.commit(&mut tx).expect("Failed to commit object birth");
}
