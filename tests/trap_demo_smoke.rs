//! TRAP Batch 2 smoke demo — 用新参数块 ABI 实际走一遍复杂服务。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::memory::Memory;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn u16_le(v: u16) -> [u8; 2] { v.to_le_bytes() }
fn u32_le(v: u32) -> [u8; 4] { v.to_le_bytes() }
fn u64_le(v: u64) -> [u8; 8] { v.to_le_bytes() }

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

fn birth(kernel: &Kernel) -> u64 {
    let mut ctx = kernel.test_begin();
    let id = match kernel.handle(
        &mut ctx,
        KernelCall::ObjectBirth { object_type: ObjectType::StateObject },
    ) {
        TrapResult::ObjectId(id) => id,
        other => panic!("birth: {:?}", other),
    };
    kernel.handle(&mut ctx, KernelCall::Commit);
    id
}

#[test]
fn trap_demo_savepoint_via_param_block() {
    let kernel = Kernel::new();
    let _obj = birth(&kernel);

    // 通过 TRAP 参数块调用 Savepoint
    let mut mem = Memory::new(256);
    let block = name_block("demo-sp");
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(7, 0, 0, 0, &mem).unwrap();
    match &call {
        KernelCall::Savepoint { name } => assert_eq!(name, "demo-sp"),
        other => panic!("expected Savepoint, got {:?}", other),
    }

    // 实际执行
    let mut ctx = kernel.test_begin();
    let result = kernel.handle(&mut ctx, call);
    assert!(matches!(result, TrapResult::Success));
    println!("[demo] Savepoint via TRAP param block OK");
}

#[test]
fn trap_demo_capability_grant_via_param_block() {
    let kernel = Kernel::new();
    let a = birth(&kernel);
    let b = birth(&kernel);

    // 通过 TRAP 参数块调用 CapabilityGrant（grantor=a, grantee=b, resource=a）
    let mut mem = Memory::new(256);
    let block = grant_block(a, b, "read", a);
    mem.write_bytes(0, &block).unwrap();
    let call = KernelCall::decode_with_memory(9, 0, 0, 0, &mem).unwrap();
    match &call {
        KernelCall::CapabilityGrant { grantor, grantee, capability_type, resource } => {
            assert_eq!(*grantor, a);
            assert_eq!(*grantee, b);
            assert_eq!(capability_type, "read");
            assert_eq!(*resource, a);
        }
        other => panic!("expected CapabilityGrant, got {:?}", other),
    }

    // 实际执行
    let mut ctx = kernel.test_begin();
    let result = kernel.handle(&mut ctx, call);
    match result {
        TrapResult::CapabilityId(cap_id) => {
            assert!(cap_id > 0);
            println!("[demo] CapabilityGrant via TRAP param block OK, cap_id={}", cap_id);
        }
        other => panic!("expected CapabilityId, got {:?}", other),
    }
}
