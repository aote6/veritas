//! Kernel World Runtime：跨 Module 通过 Runtime 执行可见性。
//!
//! 验证内容：Module A 的 Object 通过 runtime execute 对 Module B 可见。
//! 对应 VERIFICATION_MAP：kernel_world_runtime.rs
//! 若失败，意味着跨模块运行时可见性或执行路径断裂。

use std::sync::Arc;
use veritas_kernel::instruction::Instruction;
use veritas_kernel::kernel::{Kernel, KernelCall};
use veritas_kernel::module::{ModuleImage, ModuleVersion};
use veritas_kernel::program::ProgramImage;
use veritas_kernel::runtime::Runtime;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectState;

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

/// Module A 对象经 runtime execute 对 Module B 可见。
/// 失败意味着跨模块执行/可见性不变量被破坏。
/// @category: D
/// @layer: integration
/// @testworld: NOT_USED
/// @req: INT-02
#[test]
fn module_a_object_visible_to_module_b_through_runtime_execute() {
    let kernel = Arc::new(Kernel::new());

    let module_a = make_birth_module();
    let object_id = match Runtime::execute(&kernel, &module_a).expect("module A execute failed") {
        veritas_kernel::runtime::ExecutionOutcome::Completed { r0, .. } => r0,
        other => panic!("expected Completed, got {:?}", other),
    };

    assert!(
        object_id > 0,
        "module A should receive ObjectId from TRAP, got {}",
        object_id
    );

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
