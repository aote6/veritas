use crate::common::{new_kernel, root_object_id};

/// O2 Invariant: Object A 与 Object B 物理内存空间彻底隔离，写入相互不侵扰
#[test]
fn o2_object_spatial_isolation() {
    let kernel = new_kernel();
    let root = root_object_id();
    let obj_a = root ^ 0xAAAA;
    let obj_b = root ^ 0xBBBB;
    let state_id = 42;

    // 1. Birth 两个独立 Object
    let mut tx_birth = kernel.begin();
    kernel.engine.object_birth(&mut tx_birth, obj_a).unwrap();
    kernel.engine.object_birth(&mut tx_birth, obj_b).unwrap();
    kernel.engine.commit(&mut tx_birth).unwrap();

    // 2. 向 Obj A 的 state_id 写入数据 [1, 2, 3]
    let mut tx_a = kernel.engine.begin_in_object(obj_a);
    kernel.engine.write(&mut tx_a, state_id, vec![1, 2, 3]).unwrap();
    kernel.engine.commit(&mut tx_a).unwrap();

    // 3. 向 Obj B 的相同 state_id 写入数据 [9, 9, 9]
    let mut tx_b = kernel.engine.begin_in_object(obj_b);
    kernel.engine.write(&mut tx_b, state_id, vec![9, 9, 9]).unwrap();
    kernel.engine.commit(&mut tx_b).unwrap();

    // 4. 断言：读取 A 不会看到 B 的脏数据
    let mut read_tx_a = kernel.engine.begin_in_object(obj_a);
    let val_a = kernel.engine.read(&mut read_tx_a, state_id).unwrap();
    assert_eq!(val_a, vec![1, 2, 3], "O2 Violation: Object A memory polluted by Object B!");
}
