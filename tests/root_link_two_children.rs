//! 验证: root 身份下连续 birth 两个子对象后，
//! 是否可以不经 CALL、直接 TRAP ObjectLink 建立两者关系。
//!
//! 依据: ObjectBirth（TRAP 0）执行时会把新对象的 self-AdminCap attach 到
//! ctx.capabilities。这个 attach 是 ctx 级别的全局状态，不区分当前身份。
//! authorize_intent 的 has_pending 分支里 ctx.capabilities.contains(cap_id)
//! 不要求 grantee 等于当前身份，所以 root 不需要 CALL 进子对象就能引用它们
//! 已经 attach 的 cap 来完成 LINK。本测试验证这个推断是否成立。
//!
//! 正式入口: TRAP service_id（非 legacy Instruction::ObjectBirth/ObjectLink/Commit）。

use std::sync::Arc;
use veritas_kernel::instruction::Instruction;
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

/// Root 可对两个自己 birth 的 child 直接 link（无需 CALL 切换）。
/// 失败意味着 self-birthed 对象的 link 授权路径被错误限制。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: OBJ-01
#[test]
fn root_can_link_two_self_birthed_children_without_call() {
    let kernel = Kernel::with_wal_path(temp_wal("root_link_two_children"));
    let mut machine = Machine::new(Arc::new(kernel));

    let mut image: Vec<u8> = Vec::new();

    // 1. TRAP 0 ObjectBirth -> A id in R0
    image.extend_from_slice(&Instruction::Trap { service_id: 0 }.encode().unwrap());
    // 2. LOAD_CONST R2, 0
    image.extend_from_slice(&Instruction::LoadConst { reg: 2, val: 0 }.encode().unwrap());
    // 3. ADD R1, R0, R2   (R1 = A id)
    image.extend_from_slice(
        &Instruction::Add {
            dst: 1,
            src1: 0,
            src2: 2,
        }
        .encode()
        .unwrap(),
    );
    // 4. TRAP 0 ObjectBirth -> B id in R0
    image.extend_from_slice(&Instruction::Trap { service_id: 0 }.encode().unwrap());
    // 5. Preserve B: ADD R3, R0, R2  (R3 = B; R2 still 0)
    image.extend_from_slice(
        &Instruction::Add {
            dst: 3,
            src1: 0,
            src2: 2,
        }
        .encode()
        .unwrap(),
    );
    // 6. R0 = A: ADD R0, R1, R2
    image.extend_from_slice(
        &Instruction::Add {
            dst: 0,
            src1: 1,
            src2: 2,
        }
        .encode()
        .unwrap(),
    );
    // 7. R1 = B: ADD R1, R3, R2
    image.extend_from_slice(
        &Instruction::Add {
            dst: 1,
            src1: 3,
            src2: 2,
        }
        .encode()
        .unwrap(),
    );
    // 8. R2 = Owns (1)
    image.extend_from_slice(&Instruction::LoadConst { reg: 2, val: 1 }.encode().unwrap());
    // 9. TRAP 2 ObjectLink (R0=from, R1=to, R2=link_type)
    image.extend_from_slice(&Instruction::Trap { service_id: 2 }.encode().unwrap());
    // 10. TRAP 5 Commit
    image.extend_from_slice(&Instruction::Trap { service_id: 5 }.encode().unwrap());
    // 11. HALT
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());

    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    let step_names = [
        "TRAP0_BIRTH(A)",
        "LOAD_CONST",
        "ADD_R1",
        "TRAP0_BIRTH(B)",
        "ADD_R3",
        "ADD_R0",
        "ADD_R1",
        "LOAD_CONST_LINKTYPE",
        "TRAP2_LINK",
        "TRAP5_COMMIT",
        "HALT",
    ];
    for name in step_names.iter() {
        machine.step().unwrap();
        assert_not_trapped(&machine, name);
        if matches!(machine.status(), MachineStatus::Halted) {
            break;
        }
    }

    assert_eq!(
        machine.current_object(),
        0,
        "root identity must remain unchanged throughout (no CALL used)"
    );
}
