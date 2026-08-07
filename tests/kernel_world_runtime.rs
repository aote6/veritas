use std::sync::Arc;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::instruction::Instruction;
use veritas_kernel::kernel::{Kernel, KernelCall};
use veritas_kernel::module::{ModuleImage, ModuleVersion};
use veritas_kernel::program::ProgramImage;
use veritas_kernel::runtime::Runtime;
use veritas_kernel::types::{ObjectState, ObjectType};

/// Module A: TRAP OBJECT_BIRTH → COMMIT → HALT
fn make_birth_module() -> ModuleImage {
    let instructions = vec![
        Instruction::Trap { service_id: 0 }, // OBJECT_BIRTH
        Instruction::Commit,
        Instruction::Halt,
    ];
    let image = ProgramImage::new(instructions);
    ModuleImage::new("birth", ModuleVersion::new(1, 0, 0), image)
}

#[test]
fn module_a_object_visible_to_module_b_through_runtime_execute() {
    let kernel = Arc::new(Kernel::new());

    let module_a = make_birth_module();
    let (_pc_a, object_id) = Runtime::execute(&kernel, &module_a)
        .expect("module A execute failed");

    assert!(object_id > 0, "module A should receive ObjectId from TRAP, got {}", object_id);

    // Cross-execute visibility: object created by module A is Alive in shared Kernel world
    assert_eq!(
        kernel.get_object_state(object_id),
        Some(ObjectState::Alive),
        "object from module A must be visible after execute returns"
    );

    // Freeze requires acting as the target (AccessIntent::Freeze self-access)
    let mut ctx = kernel.test_begin_in_object(object_id);
    kernel
        .handle(&mut ctx, KernelCall::ObjectFreeze { object_id })
        .expect("freeze as self must succeed");
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();

    assert_eq!(
        kernel.get_object_state(object_id),
        Some(ObjectState::Frozen),
        "object should be frozen after authorized OBJECT_FREEZE"
    );
}
