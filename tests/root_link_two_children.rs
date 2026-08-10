//! 验证: root 身份下连续 birth 两个子对象后，
//! 是否可以不经 CALL、直接 OBJECT_LINK 建立两者关系。
//!
//! 依据: OBJECT_BIRTH 执行时会把新对象的 self-AdminCap attach 到
//! ctx.capabilities（三度修正的最终解法）。这个 attach 是 ctx 级别的
//! 全局状态，不区分当前身份是谁。authorize_intent 的 has_pending 分支
//! 里 ctx.capabilities.contains(cap_id) 这个 or 条件不要求 grantee
//! 等于当前身份，所以理论上 root 不需要 CALL 进子对象就能引用它们
//! 已经 attach 的 cap 来完成 LINK。本测试验证这个推断是否成立。

use std::sync::Arc;
use veritas_kernel::instruction::{Instruction, Operand};
use veritas_kernel::types::LinkType;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::machine::{Machine, MachineStatus};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn assert_not_trapped(machine: &Machine, step_name: &str) {
    if let MachineStatus::Trapped(r) = machine.status() {
        panic!("{} should not trap, got {:?}", step_name, r);
    }
}

#[test]
fn root_can_link_two_self_birthed_children_without_call() {
    let kernel = Kernel::with_wal_path(temp_wal("root_link_two_children"));
    let mut machine = Machine::new(Arc::new(kernel));

    let mut image: Vec<u8> = Vec::new();

    // 1. OBJECT_BIRTH 0  -> A id in R0
    image.extend_from_slice(&Instruction::ObjectBirth { object_id: 0 }.encode().unwrap());
    // 2. LOAD_CONST R2, 0
    image.extend_from_slice(&Instruction::LoadConst { reg: 2, val: 0 }.encode().unwrap());
    // 3. ADD R1, R0, R2   (R1 = A id, preserved before next birth overwrites R0)
    image.extend_from_slice(&Instruction::Add { dst: 1, src1: 0, src2: 2 }.encode().unwrap());
    // 4. OBJECT_BIRTH 0  -> B id in R0
    image.extend_from_slice(&Instruction::ObjectBirth { object_id: 0 }.encode().unwrap());
    // 5. OBJECT_LINK R1, R0, owns   (A -> B)
    image.extend_from_slice(&Instruction::ObjectLink {
        from: Operand::Register(1),
        to: Operand::Register(0),
        relation: LinkType::Owns,
    }.encode().unwrap());
    // 6. COMMIT
    image.extend_from_slice(&Instruction::Commit.encode().unwrap());
    // 7. HALT
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());

    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    let step_names = ["OBJECT_BIRTH(A)", "LOAD_CONST", "ADD", "OBJECT_BIRTH(B)", "OBJECT_LINK", "COMMIT", "HALT"];
    for name in step_names.iter() {
        machine.step().unwrap();
        assert_not_trapped(&machine, name);
        if matches!(machine.status(), MachineStatus::Halted) {
            break;
        }
    }

    assert_eq!(machine.current_object(), 0, "root identity must remain unchanged throughout (no CALL used)");
}
