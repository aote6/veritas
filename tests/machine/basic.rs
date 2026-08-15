//! E2E-1~4：完整闭环执行验证。
//!
//! 不使用 Runtime::execute（is_halted() 不识别 Trapped 状态，
//! 遇到 Trap 会死循环——见 docs/VASM_EXECUTION_MODEL.md "已知问题"节），
//! 改用带步数上限的手动 step 循环。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use veritas_kernel::assembler::assemble_module;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::machine::{Machine, MachineStatus};

fn fresh_wal(tag: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = format!("./.test_wal_{}_{}_{}.log", tag, std::process::id(), n);
    let _ = std::fs::remove_file(&path);
    path
}

fn run_bounded(machine: &mut Machine, max_steps: usize) {
    let mut i = 0;
    loop {
        match machine.status() {
            MachineStatus::Halted | MachineStatus::Aborted(_) | MachineStatus::Trapped(_) => break,
            _ => {}
        }
        machine.step().expect("step() itself must not error");
        i += 1;
        assert!(
            i < max_steps,
            "machine did not reach a terminal status within {} steps (possible infinite loop)",
            max_steps
        );
    }
}

/// E2E-1: 单对象闭环 — birth -> CALL -> WRITE -> RETURN -> COMMIT -> HALT
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03
#[test]
fn e2e_1_single_object_closure() {
    let src = r#"
        module e2e1
        version 1.0.0

        OBJECT_BIRTH 0
        LOAD_CONST R3, 0
        ADD R1, R0, R3
        CALL R1, body
        COMMIT
        HALT

        body:
        WRITE R0, "hello"
        RETURN
    "#;
    let m = assemble_module(src).expect("assemble e2e1");
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal("e2e1")));
    let mut machine = Machine::new(Arc::clone(&kernel));
    machine.boot(m.program_image).expect("boot e2e1");
    run_bounded(&mut machine, 100);

    assert!(
        matches!(machine.status(), MachineStatus::Halted),
        "expected Halted, got {:?}",
        machine.status()
    );
    assert_eq!(
        machine.current_object(),
        0,
        "must return to root identity after RETURN"
    );

    let obj_id = machine.registers().get_u64(1);
    assert!(
        kernel.get_object_state(obj_id).is_some(),
        "birthed object must exist after commit"
    );
}

/// E2E-2: 动态寄存器数据流 — 对象 id 经 ADD 复制到 R2 后，
/// 该寄存器值（而非原始 R0）被用作 CALL 的目标 operand。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03
#[test]
fn e2e_2_dynamic_register_dataflow() {
    let src = r#"
        module e2e2
        version 1.0.0

        OBJECT_BIRTH 0
        LOAD_CONST R5, 0
        ADD R2, R0, R5
        CALL R2, body
        COMMIT
        HALT

        body:
        WRITE R0, "written after dynamic dispatch"
        RETURN
    "#;
    let m = assemble_module(src).expect("assemble e2e2");
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal("e2e2")));
    let mut machine = Machine::new(Arc::clone(&kernel));
    machine.boot(m.program_image).expect("boot e2e2");
    run_bounded(&mut machine, 100);

    assert!(
        matches!(machine.status(), MachineStatus::Halted),
        "expected Halted, got {:?}",
        machine.status()
    );

    let obj_id_via_r0 = machine.registers().get_u64(0);
    let obj_id_via_r2 = machine.registers().get_u64(2);
    assert_eq!(
        obj_id_via_r0, obj_id_via_r2,
        "ADD-copied register must equal source register"
    );
    assert!(
        kernel.get_object_state(obj_id_via_r2).is_some(),
        "object reached via dynamic register CALL must exist"
    );
}

/// E2E-3: Birth + Write + Link 全链路 — 直接跑项目根目录的 world_demo.vasm，
/// 保证这个测试和实际交付的 demo 文件是同一份东西，不会脱节。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03
#[test]
fn e2e_3_birth_write_link_full_chain() {
    let src = std::fs::read_to_string("world_demo.vasm")
        .expect("world_demo.vasm must exist at project root");
    let m = assemble_module(&src).expect("assemble world_demo.vasm");
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal("e2e3")));
    let mut machine = Machine::new(Arc::clone(&kernel));
    machine.boot(m.program_image).expect("boot world_demo.vasm");
    run_bounded(&mut machine, 200);

    assert!(
        matches!(machine.status(), MachineStatus::Halted),
        "world_demo.vasm must halt cleanly, got {:?}",
        machine.status()
    );
    assert_eq!(
        machine.current_object(),
        0,
        "must return to root after both RETURNs"
    );

    let a = machine.registers().get_u64(1);
    let b = machine.registers().get_u64(2);
    assert_ne!(a, b, "object A and B must be distinct");
    assert!(
        kernel.get_object_state(a).is_some(),
        "object A must exist after commit"
    );
    assert!(
        kernel.get_object_state(b).is_some(),
        "object B must exist after commit"
    );
    assert!(
        kernel.has_link(a, b),
        "A must OWN B after world_demo.vasm runs"
    );
}

/// E2E-4: 跨对象非法操作必须失败 —
/// 事务1 birth 并 commit 对象 A；事务2（全新 ctx，同一个 kernel）
/// 在从未被授予任何 capability 的情况下尝试 CALL 进 A，必须被拒绝。
/// @category: B
/// @layer: capability
/// @testworld: FORBIDDEN
/// @req: CAP-15
#[test]
fn e2e_4_illegal_cross_tx_call_denied() {
    let wal = fresh_wal("e2e4");
    let kernel = Arc::new(Kernel::with_wal_path(wal));

    let a_id = {
        let src = r#"
            module e2e4_birth
            version 1.0.0
            OBJECT_BIRTH 0
            COMMIT
            HALT
        "#;
        let m = assemble_module(src).expect("assemble e2e4 birth");
        let mut machine = Machine::new(Arc::clone(&kernel));
        machine.boot(m.program_image).expect("boot e2e4 birth");
        run_bounded(&mut machine, 50);
        assert!(matches!(machine.status(), MachineStatus::Halted));
        machine.registers().get_u64(0)
    };
    assert!(
        kernel.get_object_state(a_id).is_some(),
        "object A must be committed from tx1"
    );

    {
        let src = format!(
            r#"
            module e2e4_attack
            version 1.0.0
            LOAD_CONST R0, {}
            CALL R0, 0
            HALT
            "#,
            a_id
        );
        let m = assemble_module(&src).expect("assemble e2e4 attack");
        let mut machine = Machine::new(Arc::clone(&kernel));
        machine.boot(m.program_image).expect("boot e2e4 attack");
        run_bounded(&mut machine, 50);

        match machine.status() {
            MachineStatus::Trapped(_) => {}
            other => panic!(
                "CALL into object without capability must Trap, got {:?}",
                other
            ),
        }
        assert_eq!(
            machine.current_object(),
            0,
            "identity must NOT switch on a denied CALL"
        );
    }
}
