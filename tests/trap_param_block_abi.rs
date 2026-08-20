//! TRAP parameter-block ABI decoder tests (service_id 6–12).
//!
//! Layer 1: successful decode produces exact KernelCall fields.
//! Layer 2: malformed blocks / OOB fail-closed (Err, no panic).

use veritas_kernel::kernel::KernelCall;
use veritas_kernel::memory::Memory;

fn mem_with(bytes: &[u8]) -> Memory {
    let mut m = Memory::new(4096);
    m.write_bytes(0, bytes).expect("write");
    m
}

fn mem_at(addr: usize, bytes: &[u8]) -> Memory {
    let mut m = Memory::new(4096.max(addr + bytes.len() + 64));
    m.write_bytes(addr, bytes).expect("write");
    m
}

fn u16_le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}
fn u32_le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}
fn u64_le(v: u64) -> [u8; 8] {
    v.to_le_bytes()
}

// ===== Effect (6) =====

/// ABI/semantic test: effect_decode_normal_payload
#[test]
fn effect_decode_normal_payload() {
    let payload = b"hello-effect";
    let total = 7 + payload.len();
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(1); // field_count
    buf.extend_from_slice(&u32_le(payload.len() as u32));
    buf.extend_from_slice(payload);
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(6, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::Effect { payload: p } => assert_eq!(p, payload),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: effect_decode_empty_payload
#[test]
fn effect_decode_empty_payload() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(7));
    buf.push(1);
    buf.extend_from_slice(&u32_le(0));
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(6, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::Effect { payload } => assert!(payload.is_empty()),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: effect_malformed_field_count
#[test]
fn effect_malformed_field_count() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(7));
    buf.push(2); // wrong
    buf.extend_from_slice(&u32_le(0));
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(6, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: effect_malformed_total_len_mismatch
#[test]
fn effect_malformed_total_len_mismatch() {
    // total_len claims 20 but only 7+3 bytes of structure with payload_len=3
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(20));
    buf.push(1);
    buf.extend_from_slice(&u32_le(3));
    buf.extend_from_slice(b"abc");
    // pad to 20 so RAM read succeeds but structure check fails
    while buf.len() < 20 {
        buf.push(0);
    }
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(6, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: effect_malformed_payload_oob
#[test]
fn effect_malformed_payload_oob() {
    // total_len says 7 but payload_len claims 100
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(7));
    buf.push(1);
    buf.extend_from_slice(&u32_le(100));
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(6, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: effect_addr_oob
#[test]
fn effect_addr_oob() {
    let m = Memory::new(64);
    assert!(KernelCall::decode_with_memory(6, 1000, 0, 0, &m).is_err());
}

// ===== Savepoint (7) / RollbackTo (8) =====

fn name_block(name: &str) -> Vec<u8> {
    let nb = name.as_bytes();
    let total = 5 + nb.len();
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(1);
    buf.extend_from_slice(&u16_le(nb.len() as u16));
    buf.extend_from_slice(nb);
    buf
}

/// ABI/semantic test: savepoint_decode_normal_name
#[test]
fn savepoint_decode_normal_name() {
    let buf = name_block("sp1");
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(7, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::Savepoint { name } => assert_eq!(name, "sp1"),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: savepoint_decode_empty_name
#[test]
fn savepoint_decode_empty_name() {
    let buf = name_block("");
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(7, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::Savepoint { name } => assert_eq!(name, ""),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: rollback_decode_normal_name
#[test]
fn rollback_decode_normal_name() {
    let buf = name_block("rb");
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(8, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::RollbackTo { name } => assert_eq!(name, "rb"),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: savepoint_invalid_utf8
#[test]
fn savepoint_invalid_utf8() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(7)); // 5+2
    buf.push(1);
    buf.extend_from_slice(&u16_le(2));
    buf.extend_from_slice(&[0xFF, 0xFE]); // invalid UTF-8
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(7, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: savepoint_name_len_oob
#[test]
fn savepoint_name_len_oob() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(5)); // claims no name bytes
    buf.push(1);
    buf.extend_from_slice(&u16_le(10)); // but name_len=10
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(7, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: savepoint_wrong_field_count
#[test]
fn savepoint_wrong_field_count() {
    let mut buf = name_block("x");
    buf[2] = 2;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(7, 0, 0, 0, &m).is_err());
}

// ===== CapabilityGrant (9) =====

fn grant_block(grantor: u64, grantee: u64, cap_type: &str, resource: u64) -> Vec<u8> {
    let tb = cap_type.as_bytes();
    let total = 21 + tb.len() + 8;
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(4);
    buf.extend_from_slice(&u64_le(grantor));
    buf.extend_from_slice(&u64_le(grantee));
    buf.extend_from_slice(&u16_le(tb.len() as u16));
    buf.extend_from_slice(tb);
    buf.extend_from_slice(&u64_le(resource));
    buf
}

/// ABI/semantic test: capability_grant_decode_normal
#[test]
fn capability_grant_decode_normal() {
    let buf = grant_block(1, 2, "AdminCap", 3);
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(9, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::CapabilityGrant {
            grantor,
            grantee,
            capability_type,
            resource,
        } => {
            assert_eq!(grantor, 1);
            assert_eq!(grantee, 2);
            assert_eq!(capability_type, "AdminCap");
            assert_eq!(resource, 3);
        }
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: capability_grant_type_oob
#[test]
fn capability_grant_type_oob() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(21)); // minimal without type/resource
    buf.push(4);
    buf.extend_from_slice(&u64_le(1));
    buf.extend_from_slice(&u64_le(2));
    buf.extend_from_slice(&u16_le(100)); // type_len too large
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(9, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_grant_invalid_utf8
#[test]
fn capability_grant_invalid_utf8() {
    let mut buf = Vec::new();
    let type_len = 2usize;
    let total = 21 + type_len + 8;
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(4);
    buf.extend_from_slice(&u64_le(1));
    buf.extend_from_slice(&u64_le(2));
    buf.extend_from_slice(&u16_le(type_len as u16));
    buf.extend_from_slice(&[0x80, 0xFF]);
    buf.extend_from_slice(&u64_le(3));
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(9, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_grant_wrong_field_count
#[test]
fn capability_grant_wrong_field_count() {
    let mut buf = grant_block(1, 2, "X", 3);
    buf[2] = 3;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(9, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_grant_missing_resource
#[test]
fn capability_grant_missing_resource() {
    // total_len stops before resource
    let tb = b"AdminCap";
    let total = 21 + tb.len(); // no resource
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(total as u16));
    buf.push(4);
    buf.extend_from_slice(&u64_le(1));
    buf.extend_from_slice(&u64_le(2));
    buf.extend_from_slice(&u16_le(tb.len() as u16));
    buf.extend_from_slice(tb);
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(9, 0, 0, 0, &m).is_err());
}

// ===== CapabilityRevoke (10) =====

fn revoke_block(cap_id: u64, holder: u64, tag: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&u16_le(20));
    buf.push(3);
    buf.extend_from_slice(&u64_le(cap_id));
    buf.extend_from_slice(&u64_le(holder));
    buf.push(tag);
    buf
}

/// ABI/semantic test: capability_revoke_decode_tags
#[test]
fn capability_revoke_decode_tags() {
    for (tag, expected) in [(0, None), (1, Some(false)), (2, Some(true))] {
        let buf = revoke_block(42, 7, tag);
        let m = mem_with(&buf);
        let call = KernelCall::decode_with_memory(10, 0, 0, 0, &m).unwrap();
        match call {
            KernelCall::CapabilityRevoke {
                capability_id,
                holder,
                cascade_override,
            } => {
                assert_eq!(capability_id, 42);
                assert_eq!(holder, 7);
                assert_eq!(cascade_override, expected);
            }
            other => panic!("unexpected {:?}", other),
        }
    }
}

/// ABI/semantic test: capability_revoke_illegal_tag
#[test]
fn capability_revoke_illegal_tag() {
    let buf = revoke_block(1, 2, 3);
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(10, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_revoke_wrong_total_len
#[test]
fn capability_revoke_wrong_total_len() {
    let mut buf = revoke_block(1, 2, 0);
    buf[0] = 19; // total_len=19
    buf[1] = 0;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(10, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_revoke_wrong_field_count
#[test]
fn capability_revoke_wrong_field_count() {
    let mut buf = revoke_block(1, 2, 0);
    buf[2] = 2;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(10, 0, 0, 0, &m).is_err());
}

// ===== CapabilityDelegate (11) =====

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

/// ABI/semantic test: capability_delegate_decode_normal
#[test]
fn capability_delegate_decode_normal() {
    let buf = delegate_block(9, 1, 2, 1);
    let m = mem_with(&buf);
    let call = KernelCall::decode_with_memory(11, 0, 0, 0, &m).unwrap();
    match call {
        KernelCall::CapabilityDelegate {
            capability_id,
            from,
            to,
            cascade_on_revoke,
        } => {
            assert_eq!(capability_id, 9);
            assert_eq!(from, 1);
            assert_eq!(to, 2);
            assert!(cascade_on_revoke);
        }
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: capability_delegate_illegal_bool
#[test]
fn capability_delegate_illegal_bool() {
    let buf = delegate_block(1, 2, 3, 2);
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(11, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_delegate_wrong_total_len
#[test]
fn capability_delegate_wrong_total_len() {
    let mut buf = delegate_block(1, 2, 3, 0);
    buf[0] = 27;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(11, 0, 0, 0, &m).is_err());
}

/// ABI/semantic test: capability_delegate_wrong_field_count
#[test]
fn capability_delegate_wrong_field_count() {
    let mut buf = delegate_block(1, 2, 3, 0);
    buf[2] = 3;
    let m = mem_with(&buf);
    assert!(KernelCall::decode_with_memory(11, 0, 0, 0, &m).is_err());
}

// ===== MemoryAlloc (12) register ABI =====

/// ABI/semantic test: memory_alloc_register_decode
#[test]
fn memory_alloc_register_decode() {
    let m = Memory::new(16);
    let call = KernelCall::decode_with_memory(12, 100, 64, 0, &m).unwrap();
    match call {
        KernelCall::MemoryAlloc {
            object_id,
            size_hint,
        } => {
            assert_eq!(object_id, 100);
            assert_eq!(size_hint, 64);
        }
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: decode_without_memory_rejects_complex
#[test]
fn decode_without_memory_rejects_complex() {
    assert!(KernelCall::decode(6, 0, 0, 0).is_err());
    assert!(KernelCall::decode(9, 0, 0, 0).is_err());
}

/// ABI/semantic test: param_block_at_nonzero_offset
#[test]
fn param_block_at_nonzero_offset() {
    let buf = name_block("mid");
    let m = mem_at(100, &buf);
    let call = KernelCall::decode_with_memory(7, 100, 0, 0, &m).unwrap();
    match call {
        KernelCall::Savepoint { name } => assert_eq!(name, "mid"),
        other => panic!("unexpected {:?}", other),
    }
}

/// ABI/semantic test: param_block_r0_at_ram_end
#[test]
fn param_block_r0_at_ram_end() {
    let m = Memory::new(8);
    // addr == len → OOB
    assert!(KernelCall::decode_with_memory(6, 8, 0, 0, &m).is_err());
}
