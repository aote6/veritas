//! TRAP ↔ KernelCall semantic equivalence for complex services (6–12).
//!
//! Proves: decode_with_memory → Kernel::handle produces the same transaction
//! state as constructing KernelCall directly (TRAP is entry only, not new semantics).

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::memory::Memory;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn u16_le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}
fn u32_le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}
fn u64_le(v: u64) -> [u8; 8] {
    v.to_le_bytes()
}

fn effect_block(payload: &[u8]) -> Vec<u8> {
    let total = 7 + payload.len();
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(1);
    buf.extend_from_slice(&u32_le(payload.len() as u32));
    buf.extend_from_slice(payload);
    buf
}

fn name_block(name: &str) -> Vec<u8> {
    let nb = name.as_bytes();
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le((5 + nb.len()) as u16));
    buf.push(1);
    buf.extend_from_slice(&u16_le(nb.len() as u16));
    buf.extend_from_slice(nb);
    buf
}

fn grant_block(grantor: u64, grantee: u64, cap_type: &str, resource: u64) -> Vec<u8> {
    let tb = cap_type.as_bytes();
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le((21 + tb.len() + 8) as u16));
    buf.push(4);
    buf.extend_from_slice(&u64_le(grantor));
    buf.extend_from_slice(&u64_le(grantee));
    buf.extend_from_slice(&u16_le(tb.len() as u16));
    buf.extend_from_slice(tb);
    buf.extend_from_slice(&u64_le(resource));
    buf
}

fn revoke_block(cap_id: u64, holder: u64, tag: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(20));
    buf.push(3);
    buf.extend_from_slice(&u64_le(cap_id));
    buf.extend_from_slice(&u64_le(holder));
    buf.push(tag);
    buf
}

fn delegate_block(cap_id: u64, from: u64, to: u64, cascade: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(28));
    buf.push(4);
    buf.extend_from_slice(&u64_le(cap_id));
    buf.extend_from_slice(&u64_le(from));
    buf.extend_from_slice(&u64_le(to));
    buf.push(cascade);
    buf
}

fn birth(kernel: &Kernel) -> u64 {
    let mut ctx = kernel.test_begin();
    let id = match kernel.handle(
        &mut ctx,
        KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        },
    ) {
        TrapResult::ObjectId(id) => id,
        _ => panic!("birth"),
    };
    kernel.handle(&mut ctx, KernelCall::Commit);
    id
}

// ----- Effect -----

/// ABI/semantic test: trap_effect_matches_direct_kernel_call
#[test]
fn trap_effect_matches_direct_kernel_call() {
    let kernel = Kernel::new();
    let payload = b"trap-effect-payload".to_vec();

    let mut ctx_direct = kernel.test_begin();
    let direct = kernel.handle(
        &mut ctx_direct,
        KernelCall::Effect {
            payload: payload.clone(),
        },
    );
    let key_direct = match &direct {
        TrapResult::EffectKey(k) => k.clone(),
        other => panic!("direct {:?}", other),
    };

    let mut mem = Memory::new(256);
    let block = effect_block(&payload);
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(6, 0, 0, 0, &mem).unwrap();
    let mut ctx_trap = kernel.test_begin();
    let trap = kernel.handle(&mut ctx_trap, call);
    let key_trap = match &trap {
        TrapResult::EffectKey(k) => k.clone(),
        other => panic!("trap {:?}", other),
    };

    // Same tx_id space may differ; keys share format tx-seq. Compare payloads.
    assert_eq!(ctx_direct.effect_queue.len(), 1);
    assert_eq!(ctx_trap.effect_queue.len(), 1);
    assert_eq!(ctx_direct.effect_queue.effects[0].payload, payload);
    assert_eq!(ctx_trap.effect_queue.effects[0].payload, payload);
    assert_eq!(
        ctx_direct.effect_queue.effects[0].payload,
        ctx_trap.effect_queue.effects[0].payload
    );
    // Keys from identical seq=0 path share suffix pattern
    assert!(key_direct.ends_with("-0"));
    assert!(key_trap.ends_with("-0"));
    assert_eq!(
        key_direct,
        ctx_direct.effect_queue.effects[0].idempotency_key
    );
    assert_eq!(key_trap, ctx_trap.effect_queue.effects[0].idempotency_key);
}

// ----- Savepoint / RollbackTo -----

/// ABI/semantic test: trap_savepoint_and_rollback_match_direct
#[test]
fn trap_savepoint_and_rollback_match_direct() {
    let kernel = Kernel::new();
    let mut mem = Memory::new(256);

    // Direct path
    let mut ctx_d = kernel.test_begin();
    kernel
        .handle(
            &mut ctx_d,
            KernelCall::Savepoint {
                name: "s".into(),
            },
        );
    assert_eq!(ctx_d.savepoints.len(), 1);
    kernel
        .handle(
            &mut ctx_d,
            KernelCall::Effect {
                payload: b"after".to_vec(),
            },
        );
    assert_eq!(ctx_d.effect_queue.len(), 1);
    kernel
        .handle(
            &mut ctx_d,
            KernelCall::RollbackTo {
                name: "s".into(),
            },
        );
    assert_eq!(ctx_d.effect_queue.len(), 0);
    assert_eq!(ctx_d.savepoints.len(), 1);

    // TRAP path
    let mut ctx_t = kernel.test_begin();
    let sp = name_block("s");
    mem.write_bytes(0, &sp).unwrap();
    let call_sp = KernelCall::decode_with_memory(7, 0, 0, 0, &mem).unwrap();
    kernel.handle(&mut ctx_t, call_sp);
    assert_eq!(ctx_t.savepoints.len(), 1);
    assert_eq!(ctx_t.savepoints[0].name, "s");

    kernel
        .handle(
            &mut ctx_t,
            KernelCall::Effect {
                payload: b"after".to_vec(),
            },
        );
    assert_eq!(ctx_t.effect_queue.len(), 1);

    let rb = name_block("s");
    mem.write_bytes(64, &rb).unwrap();
    let call_rb = KernelCall::decode_with_memory(8, 64, 0, 0, &mem).unwrap();
    kernel.handle(&mut ctx_t, call_rb);
    assert_eq!(ctx_t.effect_queue.len(), 0);
    assert_eq!(ctx_t.savepoints.len(), 1);
}

// ----- CapabilityGrant -----

/// ABI/semantic test: trap_capability_grant_matches_direct_with_same_grantor
#[test]
fn trap_capability_grant_matches_direct_with_same_grantor() {
    let kernel = Kernel::new();
    // Birth object; ObjectBirth installs root AdminCap on the new object for itself.
    let obj = birth(&kernel);
    // Grant from obj (grantor == current_object style) to a second object.
    let obj2 = birth(&kernel);

    let mut ctx_d = kernel.test_begin_in_object(obj);
    let direct = kernel.handle(
        &mut ctx_d,
        KernelCall::CapabilityGrant {
            grantor: obj,
            grantee: obj2,
            capability_type: "ReadCap".into(),
            resource: obj,
        },
    );
    let cap_d = match direct {
        TrapResult::CapabilityId(id) => id,
        other => panic!("direct grant {:?}", other),
    };
    assert_eq!(ctx_d.pending_capabilities.len(), 1);
    assert_eq!(ctx_d.pending_capabilities[0].capability_id, cap_d);
    assert_eq!(ctx_d.pending_capabilities[0].grantee, obj2);

    let mut mem = Memory::new(256);
    let block = grant_block(obj, obj2, "ReadCap", obj);
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(9, 0, 0, 0, &mem).unwrap();
    let mut ctx_t = kernel.test_begin_in_object(obj);
    let trap = kernel.handle(&mut ctx_t, call);
    let cap_t = match trap {
        TrapResult::CapabilityId(id) => id,
        other => panic!("trap grant {:?}", other),
    };
    assert_eq!(ctx_t.pending_capabilities.len(), 1);
    assert_eq!(ctx_t.pending_capabilities[0].capability_id, cap_t);
    assert_eq!(ctx_t.pending_capabilities[0].grantor, obj);
    assert_eq!(ctx_t.pending_capabilities[0].grantee, obj2);
    assert_eq!(ctx_t.pending_capabilities[0].cap_type, "ReadCap");
    assert_eq!(ctx_t.pending_capabilities[0].resource, obj);
    // Deterministic id formula → same inputs same id within identical seq state
    assert_eq!(cap_d, cap_t);
}

// ----- CapabilityRevoke (pending grant path) -----

/// ABI/semantic test: trap_capability_revoke_pending_grant
#[test]
fn trap_capability_revoke_pending_grant() {
    let kernel = Kernel::new();
    let obj = birth(&kernel);
    let obj2 = birth(&kernel);

    let mut ctx = kernel.test_begin_in_object(obj);
    let cap_id = match kernel.handle(
        &mut ctx,
        KernelCall::CapabilityGrant {
            grantor: obj,
            grantee: obj2,
            capability_type: "ReadCap".into(),
            resource: obj,
        },
    ) {
        TrapResult::CapabilityId(id) => id,
        other => panic!("{:?}", other),
    };
    assert_eq!(ctx.pending_capabilities.len(), 1);

    let mut mem = Memory::new(64);
    // cascade_tag=0 (None)
    let block = revoke_block(cap_id, obj2, 0);
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(10, 0, 0, 0, &mem).unwrap();
    match kernel.handle(&mut ctx, call) {
        TrapResult::Success => {}
        other => panic!("{:?}", other),
    }
    assert!(
        ctx.pending_capabilities.is_empty(),
        "pending grant must be removed by revoke"
    );
}

// ----- CapabilityDelegate -----

/// ABI/semantic test: trap_capability_delegate_pending
#[test]
fn trap_capability_delegate_pending() {
    let kernel = Kernel::new();
    let obj = birth(&kernel);
    let obj2 = birth(&kernel);
    let obj3 = birth(&kernel);

    let mut ctx = kernel.test_begin_in_object(obj);
    let cap_id = match kernel.handle(
        &mut ctx,
        KernelCall::CapabilityGrant {
            grantor: obj,
            grantee: obj2,
            capability_type: "ReadCap".into(),
            resource: obj,
        },
    ) {
        TrapResult::CapabilityId(id) => id,
        other => panic!("{:?}", other),
    };

    let mut mem = Memory::new(64);
    let block = delegate_block(cap_id, obj2, obj3, 1);
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(11, 0, 0, 0, &mem).unwrap();
    match kernel.handle(&mut ctx, call) {
        TrapResult::Success => {}
        other => panic!("{:?}", other),
    }
    assert_eq!(ctx.pending_delegates.len(), 1);
    assert_eq!(ctx.pending_delegates[0].capability_id, cap_id);
    assert_eq!(ctx.pending_delegates[0].from, obj2);
    assert_eq!(ctx.pending_delegates[0].to, obj3);
    assert!(ctx.pending_delegates[0].cascade_on_revoke);
}

// ----- MemoryAlloc -----

/// ABI/semantic test: trap_memory_alloc_matches_direct
#[test]
fn trap_memory_alloc_matches_direct() {
    let kernel = Kernel::new();
    let obj = birth(&kernel);

    let mut ctx_d = kernel.test_begin_in_object(obj);
    let sid_d = match kernel.handle(
        &mut ctx_d,
        KernelCall::MemoryAlloc {
            object_id: obj,
            size_hint: 32,
        },
    ) {
        TrapResult::StateId(id) => id,
        other => panic!("{:?}", other),
    };
    assert_eq!(ctx_d.allocated_slots, vec![(obj, sid_d)]);

    let m = Memory::new(16);
    let call = KernelCall::decode_with_memory(12, obj, 32, 0, &m).unwrap();
    let mut ctx_t = kernel.test_begin_in_object(obj);
    let sid_t = match kernel.handle(&mut ctx_t, call) {
        TrapResult::StateId(id) => id,
        other => panic!("{:?}", other),
    };
    assert_eq!(sid_d, sid_t);
    assert_eq!(ctx_t.allocated_slots, vec![(obj, sid_t)]);
}

/// Machine-level: malformed TRAP yields Trapped(InvalidEncoding), not panic.
#[test]
fn machine_malformed_trap_invalid_encoding() {
    use std::sync::Arc;
    use veritas_kernel::assembler::assemble_module;
    use veritas_kernel::machine::{Machine, MachineStatus};
    use veritas_kernel::types::TrapReason;

    // TRAP service 6 with R0=0 pointing at zero-filled RAM → malformed header
    let src = r#"
        module trap_malformed
        version 1.0.0
        LOAD_CONST R0, 0
        TRAP 6
        HALT
    "#;
    let m = assemble_module(src).expect("assemble");
    let kernel = Arc::new(Kernel::new());
    let mut machine = Machine::new(Arc::clone(&kernel));
    machine.boot(m.program_image).expect("boot");
    for _ in 0..20 {
        match machine.status() {
            MachineStatus::Halted | MachineStatus::Trapped(_) | MachineStatus::Aborted(_) => break,
            _ => {
                machine.step().expect("step");
            }
        }
    }
    match machine.status() {
        MachineStatus::Trapped(TrapReason::InvalidEncoding { .. }) => {}
        other => panic!("expected InvalidEncoding trap, got {:?}", other),
    }
}
