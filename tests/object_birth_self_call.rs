//! 回归测试：OBJECT_BIRTH 不再自动切换身份（enter_object(id) 已删除），
//! 但同一事务内，创建者必须能够用 CALL 显式进入自己刚创建的对象。
//!
//! 这锁死本轮争论的最终结论：不恢复隐式身份切换，而是让 OBJECT_BIRTH
//! 把新对象的 self-AdminCap attach 到 ctx，使 CALL 这条唯一合法的
//! 身份切换入口能够真正走通 authorize_intent 审计。
//!
//! 特别覆盖 root 身份（current_object == 0）这个此前被认为"拿不到
//! 权限"的死结场景：object_birth 对 root 创建者不额外发 grant，
//! 但新对象的 self-AdminCap 始终存在且现在会被 attach，
//! 所以 root 同样能够 CALL 进它刚创建的对象。

use std::sync::Arc;
use veritas_kernel::instruction::{Instruction, Operand};
use veritas_kernel::kernel::Kernel;
use veritas_kernel::machine::{Machine, MachineStatus};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

/// root(current_object == 0) birth 一个新对象后，用 CALL 显式进入它，
/// 必须成功——即使 root 从未被 object_birth 额外授予任何 capability，
/// 新对象的 self-AdminCap 也应当已被 attach 到本事务 ctx。
#[test]
fn root_can_call_into_object_it_just_birthed() {
    let kernel = Kernel::with_wal_path(temp_wal("birth_self_call_root"));
    let mut machine = Machine::new(Arc::new(kernel));
    // machine 默认以 current_object == 0 (root) 启动，不显式调用 set_execution_object。

    let birth_bytes = Instruction::ObjectBirth { object_id: 0 }.encode().unwrap();
    assert_eq!(birth_bytes.len(), 9);

    let mut image = birth_bytes;
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());
    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    // 执行 OBJECT_BIRTH
    machine.step().unwrap();
    match machine.status() {
        MachineStatus::Trapped(r) => panic!("OBJECT_BIRTH should not trap, got {:?}", r),
        _ => {}
    }
    // 身份不应切换：OBJECT_BIRTH 不再自动 enter_object。
    assert_eq!(machine.current_object(), 0, "creator identity must remain unchanged after birth");

    let new_id = machine.registers().get_u64(0);
    assert_ne!(new_id, 0, "birth must allocate a nonzero object id into R0");

    // 追加 CALL new_id, <entry_pc> 到内存里 HALT 之后的位置。
    let call_target_pc = 9 /* birth */ + 1 /* halt */;
    let call_bytes = Instruction::Call {
        object_id: Operand::Immediate(new_id),
        entry_pc: 0, // 随便跳回 0 即可，我们只关心 CALL 本身是否被拒绝
    }
    .encode()
    .unwrap();
    machine.ram_mut().write_bytes(call_target_pc, &call_bytes).unwrap();
    machine.set_pc(call_target_pc);

    machine.step().unwrap();
    match machine.status() {
        MachineStatus::Trapped(r) => panic!(
            "CALL into self-birthed object must succeed (self-AdminCap should be attached), got trapped: {:?}",
            r
        ),
        _ => {}
    }
    assert_eq!(
        machine.current_object(),
        new_id,
        "CALL must have switched current_object to the newly birthed object"
    );
}
