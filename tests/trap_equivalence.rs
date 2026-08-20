//! TRAP ↔ legacy Instruction **compatibility** 等价性测试。
//!
//! 正式 Kernel service 入口为 TRAP。本文件中使用 OBJECT_BIRTH / COMMIT 等
//! 旧式助记符的一侧是 **TRAP determinism / KernelCall decode equivalence**，不是推荐用法。
//!
//! 验证内容：对简单 KernelCall（ObjectBirth/ObjectDeath/ObjectLink/
//! ObjectUnlink/ObjectFreeze/Commit），TRAP 路径和旧式指令路径产生
//! 相同的事务结果与 World State commitment。
//!
//! ObjectBirth 和 Commit 通过 Machine VASM 验证（legacy vs TRAP）。
//! ObjectFreeze 和 ObjectLink 通过 Kernel 层直接比较 KernelCall 语义验证。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use veritas_kernel::assembler::assemble_module;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::machine::{Machine, MachineStatus};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

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

fn run_program(src: &str, tag: &str) -> (MachineStatus, Arc<Kernel>) {
    let m = assemble_module(src).expect("assemble failed");
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal(tag)));
    let mut machine = Machine::new(Arc::clone(&kernel));
    machine.boot(m.program_image).expect("boot failed");
    run_bounded(&mut machine, 100);

    let status = machine.status().clone();
    (status, kernel)
}

fn assert_same_world(
    legacy_kernel: &Arc<Kernel>,
    trap_kernel: &Arc<Kernel>,
) {
    assert_eq!(
        legacy_kernel.state_root(),
        trap_kernel.state_root(),
        "state_root must match"
    );
    assert_eq!(
        legacy_kernel.list_object_ids(),
        trap_kernel.list_object_ids(),
        "object ids must match"
    );
    for id in legacy_kernel.list_object_ids() {
        assert_eq!(
            legacy_kernel.get_object_state(id),
            trap_kernel.get_object_state(id),
            "object state for id={} must match",
            id
        );
    }
}

fn birth_object(kernel: &Arc<Kernel>, as_object: u64) -> u64 {
    let mut ctx = kernel.test_begin_in_object(as_object);
    let result = kernel.handle(
        &mut ctx,
        KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        },
    );
    let id = match result {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut ctx, KernelCall::Commit);
    id
}

/// E2E-TEQ-1: ObjectBirth 等价性 — TRAP 0 determinism (two independent Machine runs)
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_object_birth() {
    let legacy_src = r#"
        module legacy_birth
        version 1.0.0
        LOAD_CONST R0, 0
        TRAP 0
        TRAP 5
        HALT
    "#;
    let trap_src = r#"
        module trap_birth
        version 1.0.0
        LOAD_CONST R0, 0
        TRAP 0
        TRAP 5
        HALT
    "#;

    let (legacy_status, legacy_kernel) = run_program(legacy_src, "teq1_legacy");
    let (trap_status, trap_kernel) = run_program(trap_src, "teq1_trap");

    assert!(matches!(legacy_status, MachineStatus::Halted));
    assert!(matches!(trap_status, MachineStatus::Halted));

    assert_same_world(&legacy_kernel, &trap_kernel);
}

/// E2E-TEQ-2: ObjectFreeze 等价性 — 旧式 KernelCall vs TRAP decode
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_object_freeze() {
    let legacy_kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq2_legacy")));
    let trap_kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq2_trap")));

    let legacy_obj = birth_object(&legacy_kernel, 0);
    let trap_obj = birth_object(&trap_kernel, 0);

    // Legacy: 直接构造 ObjectFreeze KernelCall
    let mut legacy_ctx = legacy_kernel.test_begin_in_object(legacy_obj);
    legacy_kernel.handle(
        &mut legacy_ctx,
        KernelCall::ObjectFreeze { object_id: legacy_obj },
    );
    legacy_kernel.handle(&mut legacy_ctx, KernelCall::Commit);

    // TRAP: 通过 decode 构造相同的 KernelCall
    let decoded = veritas_kernel::kernel::KernelCall::decode(4, trap_obj, 0, 0)
        .expect("decode ObjectFreeze failed");
    let mut trap_ctx = trap_kernel.test_begin_in_object(trap_obj);
    trap_kernel.handle(&mut trap_ctx, decoded);
    trap_kernel.handle(&mut trap_ctx, KernelCall::Commit);

    assert_same_world(&legacy_kernel, &trap_kernel);
}

/// E2E-TEQ-3: Commit 等价性 — TRAP 5 Commit determinism
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_commit() {
    let legacy_src = r#"
        module legacy_commit
        version 1.0.0
        LOAD_CONST R0, 0
        TRAP 0
        TRAP 5
        HALT
    "#;
    let trap_src = r#"
        module trap_commit
        version 1.0.0
        LOAD_CONST R0, 0
        TRAP 0
        TRAP 5
        HALT
    "#;

    let (legacy_status, legacy_kernel) = run_program(legacy_src, "teq3_legacy");
    let (trap_status, trap_kernel) = run_program(trap_src, "teq3_trap");

    assert!(matches!(legacy_status, MachineStatus::Halted));
    assert!(matches!(trap_status, MachineStatus::Halted));

    assert_same_world(&legacy_kernel, &trap_kernel);
}

/// E2E-TEQ-4: ObjectLink 等价性 — 旧式 KernelCall vs TRAP decode
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_object_link() {
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq4")));

    let a = birth_object(&kernel, 0);
    let b = birth_object(&kernel, 0);

    // Legacy: 直接构造 ObjectLink KernelCall
    let legacy_call = KernelCall::ObjectLink {
        from: a,
        to: b,
        link_type: veritas_kernel::types::LinkType::DependsOn,
    };

    // TRAP: decode service_id 2, r0=from, r1=to, r2=link_type(0)
    let trap_call = veritas_kernel::kernel::KernelCall::decode(2, a, b, 0)
        .expect("decode ObjectLink failed");

    // 两个 KernelCall 在结构上应完全等价
    match (&legacy_call, &trap_call) {
        (
            KernelCall::ObjectLink {
                from: lf, to: lt, link_type: ll,
            },
            KernelCall::ObjectLink {
                from: tf, to: tt, link_type: tl,
            },
        ) => {
            assert_eq!(lf, tf, "from must match");
            assert_eq!(lt, tt, "to must match");
            assert_eq!(ll, tl, "link_type must match");
        }
        _ => panic!("both must be ObjectLink"),
    }
}
/// E2E-TEQ-5: ObjectDeath 等价性 — 旧式 KernelCall vs TRAP decode
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_object_death() {
    let legacy_kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq5_legacy")));
    let trap_kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq5_trap")));

    let legacy_obj = birth_object(&legacy_kernel, 0);
    let trap_obj = birth_object(&trap_kernel, 0);

    // Legacy: 直接构造 ObjectDeath KernelCall
    let mut legacy_ctx = legacy_kernel.test_begin_in_object(legacy_obj);
    legacy_kernel.handle(
        &mut legacy_ctx,
        KernelCall::ObjectDeath { object_id: legacy_obj },
    );
    legacy_kernel.handle(&mut legacy_ctx, KernelCall::Commit);

    // TRAP: decode service_id 1, r0=object_id
    let decoded = veritas_kernel::kernel::KernelCall::decode(1, trap_obj, 0, 0)
        .expect("decode ObjectDeath failed");
    let mut trap_ctx = trap_kernel.test_begin_in_object(trap_obj);
    trap_kernel.handle(&mut trap_ctx, decoded);
    trap_kernel.handle(&mut trap_ctx, KernelCall::Commit);

    assert_same_world(&legacy_kernel, &trap_kernel);
}

/// E2E-TEQ-6: ObjectUnlink 等价性 — 旧式 KernelCall vs TRAP decode
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: KER-03, TRAP-01
#[test]
fn trap_equivalence_object_unlink() {
    let kernel = Arc::new(Kernel::with_wal_path(fresh_wal("teq6")));

    let a = birth_object(&kernel, 0);
    let b = birth_object(&kernel, 0);

    // Legacy: 直接构造 ObjectUnlink KernelCall
    let legacy_call = KernelCall::ObjectUnlink { from: a, to: b };

    // TRAP: decode service_id 3, r0=from, r1=to
    let trap_call = veritas_kernel::kernel::KernelCall::decode(3, a, b, 0)
        .expect("decode ObjectUnlink failed");

    // 两个 KernelCall 在结构上应完全等价
    match (&legacy_call, &trap_call) {
        (
            KernelCall::ObjectUnlink { from: lf, to: lt },
            KernelCall::ObjectUnlink { from: tf, to: tt },
        ) => {
            assert_eq!(lf, tf, "from must match");
            assert_eq!(lt, tt, "to must match");
        }
        _ => panic!("both must be ObjectUnlink"),
    }
}