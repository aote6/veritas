//! P3: CALL → AccessIntent unification tests.

use std::sync::Arc;
use veritas_kernel::instruction::Instruction;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::machine::{Machine, MachineStatus};
use veritas_kernel::types::{AccessIntent, ObjectType, TrapReason};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.begin();
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

fn grant(kernel: &Kernel, grantee: u64, resource: u64) -> u64 {
    let mut tx = kernel.begin();
    let cap = match kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantee,
                capability_type: "call".to_string(),
                resource,
            },
        )
        .unwrap()
    {
        TrapResult::CapabilityId(id) => id,
        _ => panic!("expected CapabilityId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    cap
}

fn load_call_program(machine: &mut Machine, callee: u64, entry_pc: usize) {
    // CALL callee, entry_pc ; HALT at entry so callee "returns" by halt
    let bytes = Instruction::Call {
        object_id: callee,
        entry_pc,
    }
    .encode()
    .unwrap();
    let halt = Instruction::Halt.encode().unwrap();
    let mut image = bytes;
    // Pad to entry_pc with NOPs if needed — entry_pc=bytes.len() then HALT
    let entry = image.len();
    image.extend_from_slice(&halt);
    // Fix: encode Call with correct entry_pc
    let bytes = Instruction::Call {
        object_id: callee,
        entry_pc: entry,
    }
    .encode()
    .unwrap();
    let mut image = bytes;
    image.extend_from_slice(&halt);
    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);
}

/// 1. No Call capability → CALL traps AccessDenied
#[test]
fn call_without_capability_fails() {
    let kernel = Kernel::with_wal_path(temp_wal("call_no_cap"));
    let caller = birth(&kernel);
    let callee = birth(&kernel);

    let mut machine = Machine::new(Arc::new(kernel));
    machine.set_execution_object(caller);
    load_call_program(&mut machine, callee, 0);

    machine.step().unwrap();
    match machine.status() {
        MachineStatus::Trapped(TrapReason::AccessDenied { .. }) => {}
        other => panic!("expected AccessDenied trap, got {:?}", other),
    }
}

/// 2. With capability on callee resource → CALL succeeds
#[test]
fn call_with_capability_succeeds() {
    let kernel = Kernel::with_wal_path(temp_wal("call_with_cap"));
    let caller = birth(&kernel);
    let callee = birth(&kernel);
    let _cap = grant(&kernel, caller, callee);

    let mut machine = Machine::new(Arc::new(kernel));
    machine.set_execution_object(caller);
    // Rebuild program after we know objects
    let bytes = Instruction::Call {
        object_id: callee,
        entry_pc: 17, // 1+8+8 = 17
    }
    .encode()
    .unwrap();
    assert_eq!(bytes.len(), 17);
    let mut image = bytes;
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());
    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    machine.step().unwrap();
    match machine.status() {
        MachineStatus::Running | MachineStatus::Ready | MachineStatus::Halted => {}
        MachineStatus::Trapped(r) => panic!("CALL should succeed, trapped {:?}", r),
        other => panic!("unexpected status {:?}", other),
    }
    assert_eq!(machine.current_object(), callee);
}

/// 3. After Delegate, delegated holder can CALL
#[test]
fn call_after_delegate_succeeds() {
    let kernel = Kernel::with_wal_path(temp_wal("call_delegate"));
    let root = birth(&kernel);
    let delegatee = birth(&kernel);
    let callee = birth(&kernel);
    let cap = grant(&kernel, root, callee);
    {
        let mut tx = kernel.begin();
        kernel
            .handle(
                &mut tx,
                KernelCall::CapabilityDelegate {
                    capability_id: cap,
                    from: root,
                    to: delegatee,
                    cascade_on_revoke: true,
                },
            )
            .unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }

    let mut machine = Machine::new(Arc::new(kernel));
    machine.set_execution_object(delegatee);
    let bytes = Instruction::Call {
        object_id: callee,
        entry_pc: 17,
    }
    .encode()
    .unwrap();
    let mut image = bytes;
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());
    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    machine.step().unwrap();
    assert_eq!(machine.current_object(), callee);
    assert!(!matches!(
        machine.status(),
        MachineStatus::Trapped(TrapReason::AccessDenied { .. })
    ));
}

/// 4. After Revoke, CALL fails immediately
#[test]
fn call_after_revoke_fails() {
    let kernel = Kernel::with_wal_path(temp_wal("call_revoke"));
    let caller = birth(&kernel);
    let callee = birth(&kernel);
    let cap = grant(&kernel, caller, callee);

    let mut tx = kernel.begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder: caller,
                cascade_override: Some(true),
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    let mut machine = Machine::new(Arc::new(kernel));
    machine.set_execution_object(caller);
    let bytes = Instruction::Call {
        object_id: callee,
        entry_pc: 17,
    }
    .encode()
    .unwrap();
    let mut image = bytes;
    image.extend_from_slice(&Instruction::Halt.encode().unwrap());
    machine.ram_mut().write_bytes(0, &image).unwrap();
    machine.set_pc(0);

    machine.step().unwrap();
    match machine.status() {
        MachineStatus::Trapped(TrapReason::AccessDenied { .. }) => {}
        other => panic!("expected AccessDenied after revoke, got {:?}", other),
    }
}

/// 5. Checkpoint restore preserves CALL authorization
#[test]
fn call_permission_survives_checkpoint() {
    let kernel = Kernel::with_wal_path(temp_wal("call_ckpt"));
    let caller = birth(&kernel);
    let callee = birth(&kernel);
    let _cap = grant(&kernel, caller, callee);

    let snap = kernel.create_checkpoint();
    let kernel2 = Kernel::with_wal_path(temp_wal("call_ckpt2"));
    assert!(kernel2.restore_checkpoint(&snap));

    // authorize_intent via engine (same path CALL uses)
    let mut ctx = kernel2.begin_in_object(caller);
    let intent = AccessIntent::Call(callee);
    assert!(
        kernel2.engine().authorize_intent(&ctx, &intent).is_ok(),
        "CALL must remain authorized after checkpoint restore"
    );

    // Without capability on another object
    let stranger = {
        // stranger not in snapshot — use a fresh id that was never granted
        99999u64
    };
    let intent_bad = AccessIntent::Call(stranger);
    // Self-access only for current; stranger is cross-object without cap
    assert!(kernel2.engine().authorize_intent(&ctx, &intent_bad).is_err());
    let _ = &mut ctx;
}

/// 6. WAL replay preserves CALL authorization
#[test]
fn call_permission_survives_wal_replay() {
    let wal = temp_wal("call_wal");
    let kernel = Kernel::with_wal_path(wal.clone());
    let caller = birth(&kernel);
    let callee = birth(&kernel);
    let _cap = grant(&kernel, caller, callee);

    let kernel2 = Kernel::with_wal_path(wal);
    let ctx = kernel2.begin_in_object(caller);
    assert!(kernel2
        .engine()
        .authorize_intent(&ctx, &AccessIntent::Call(callee))
        .is_ok());
}

/// AccessIntent::Call is collected and verified at commit
#[test]
fn call_intent_collected_in_verify_path() {
    let kernel = Kernel::with_wal_path(temp_wal("call_collect"));
    let caller = birth(&kernel);
    let callee = birth(&kernel);
    // No grant — pending Call should fail verify_capability at commit if recorded
    let mut ctx = kernel.begin_in_object(caller);
    ctx.pending_calls.push(callee);
    // commit goes through verify_capability
    let res = kernel.handle(&mut ctx, KernelCall::Commit);
    assert!(res.is_err(), "commit with unauthorized Call intent must fail");
}

/// Self-call is exempt (structural)
#[test]
fn call_self_is_exempt() {
    let kernel = Kernel::with_wal_path(temp_wal("call_self"));
    let obj = birth(&kernel);
    let ctx = kernel.begin_in_object(obj);
    assert!(kernel
        .engine()
        .authorize_intent(&ctx, &AccessIntent::Call(obj))
        .is_ok());
}
