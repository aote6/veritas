use std::sync::Arc;
use veritas_kernel::instruction::Instruction;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::module::{ModuleImage, ModuleVersion};
use veritas_kernel::program::ProgramImage;
use veritas_kernel::runtime::Runtime;

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

/// Module B: FREEZE the object created by module A → COMMIT → HALT
fn make_freeze_module(object_id: u64) -> ModuleImage {
    let instructions = vec![
        Instruction::LoadConst { reg: 0, val: object_id },
        Instruction::Trap { service_id: 4 }, // OBJECT_FREEZE
        Instruction::Commit,
        Instruction::Halt,
    ];
    let image = ProgramImage::new(instructions);
    ModuleImage::new("freeze", ModuleVersion::new(1, 0, 0), image)
}

#[test]
fn module_a_object_visible_to_module_b_through_runtime_execute() {
    let kernel = Arc::new(Kernel::new());

    // Module A: OBJECT_BIRTH via TRAP, then COMMIT
    let module_a = make_birth_module();
    let (_pc_a, object_id) = Runtime::execute(&kernel, &module_a)
        .expect("module A execute failed");

    assert!(
        object_id > 0,
        "module A should receive ObjectId from TRAP, got {}",
        object_id
    );


    // Module B: OBJECT_FREEZE the object from module A via TRAP, then COMMIT
    let module_b = make_freeze_module(object_id);
    let (_pc_b, _result) = Runtime::execute(&kernel, &module_b)
        .expect("module B execute failed");

    // Verify: object from module A is now Frozen, visible across execute boundaries
    use veritas_kernel::types::ObjectState;
    assert_eq!(
        kernel.get_object_state(object_id),
        Some(ObjectState::Frozen),
        "object should be frozen after module B's OBJECT_FREEZE"
    );
}
