use crate::instruction::Instruction;
use crate::types::VeritasError;

pub mod opcodes {
    pub const NOP: u8 = 0x00;
    pub const LOAD_CONST: u8 = 0x01;
    pub const ADD: u8 = 0x02;
    pub const SUB: u8 = 0x03;
    pub const CMP: u8 = 0x04;
    pub const JMP: u8 = 0x05;
    pub const JZ: u8 = 0x06;
    pub const JNZ: u8 = 0x07;
    pub const JN: u8 = 0x08;
    pub const LOAD_STATE_U64: u8 = 0x10;
    pub const LOAD_STATE_BYTES: u8 = 0x11;
    pub const WRITE_REGISTER: u8 = 0x12;
    pub const COMMIT: u8 = 0x23;
    pub const ABORT: u8 = 0x24;
    pub const HALT: u8 = 0xFF;

    // Kernel reserved opcodes (P15.6.1)
    pub const READ: u8 = 0x30;
    pub const WRITE: u8 = 0x31;
    pub const EFFECT: u8 = 0x32;
    pub const OBJECT_BIRTH: u8 = 0x33;
    pub const OBJECT_DEATH: u8 = 0x34;
    pub const OBJECT_LINK: u8 = 0x35;
    pub const OBJECT_UNLINK: u8 = 0x3B;
    pub const OBJECT_FREEZE: u8 = 0x3C;
    pub const HOST_CALL: u8 = 0x40;
    pub const TRAP: u8 = 0x41;
    pub const CAPABILITY_GRANT: u8 = 0x36;
    pub const SAVEPOINT: u8 = 0x37;
    pub const ROLLBACK_TO: u8 = 0x38;
    pub const CALL: u8 = 0x39;
    pub const RETURN: u8 = 0x3A;
}

fn encode_operand(buf: &mut Vec<u8>, op: &crate::instruction::Operand) {
    match op {
        crate::instruction::Operand::Immediate(v) => {
            buf.push(0u8);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        crate::instruction::Operand::Register(r) => {
            buf.push(1u8);
            buf.extend_from_slice(&(*r as u64).to_le_bytes());
        }
    }
}

fn decode_operand(
    bytes: &[u8],
    pos: &mut usize,
) -> Result<crate::instruction::Operand, VeritasError> {
    if *pos + 9 > bytes.len() {
        return Err(VeritasError::EngineError("EOF decoding operand".into()));
    }
    let tag = bytes[*pos];
    let val = u64::from_le_bytes(bytes[*pos + 1..*pos + 9].try_into().unwrap());
    *pos += 9;
    match tag {
        0 => Ok(crate::instruction::Operand::Immediate(val)),
        1 => Ok(crate::instruction::Operand::Register(val as u8)),
        _ => Err(VeritasError::EngineError(format!(
            "Invalid operand tag: {}",
            tag
        ))),
    }
}

impl Instruction {
    pub fn encode(&self) -> Result<Vec<u8>, VeritasError> {
        let mut buf = Vec::new();
        match self {
            Instruction::Nop => buf.push(opcodes::NOP),
            Instruction::LoadConst { reg, val } => {
                buf.push(opcodes::LOAD_CONST);
                buf.push(*reg);
                buf.extend_from_slice(&val.to_le_bytes());
            }
            Instruction::Add { dst, src1, src2 } => {
                buf.push(opcodes::ADD);
                buf.push(*dst);
                buf.push(*src1);
                buf.push(*src2);
            }
            Instruction::Sub { dst, src1, src2 } => {
                buf.push(opcodes::SUB);
                buf.push(*dst);
                buf.push(*src1);
                buf.push(*src2);
            }
            Instruction::Cmp { src1, src2 } => {
                buf.push(opcodes::CMP);
                buf.push(*src1);
                buf.push(*src2);
            }
            Instruction::Jmp { target } => {
                buf.push(opcodes::JMP);
                buf.extend_from_slice(&(*target as u64).to_le_bytes());
            }
            Instruction::Jz { target } => {
                buf.push(opcodes::JZ);
                buf.extend_from_slice(&(*target as u64).to_le_bytes());
            }
            Instruction::Jnz { target } => {
                buf.push(opcodes::JNZ);
                buf.extend_from_slice(&(*target as u64).to_le_bytes());
            }
            Instruction::Jn { target } => {
                buf.push(opcodes::JN);
                buf.extend_from_slice(&(*target as u64).to_le_bytes());
            }
            Instruction::LoadStateU64 { reg, state_id } => {
                buf.push(opcodes::LOAD_STATE_U64);
                buf.push(*reg);
                buf.extend_from_slice(&state_id.to_le_bytes());
            }
            Instruction::LoadStateBytes { reg, state_id } => {
                buf.push(opcodes::LOAD_STATE_BYTES);
                buf.push(*reg);
                buf.extend_from_slice(&state_id.to_le_bytes());
            }
            Instruction::WriteRegister { state_id, reg } => {
                buf.push(opcodes::WRITE_REGISTER);
                buf.extend_from_slice(&state_id.to_le_bytes());
                buf.push(*reg);
            }
            Instruction::Commit => buf.push(opcodes::COMMIT),
            Instruction::Abort { .. } => buf.push(opcodes::ABORT),
            Instruction::Call {
                object_id,
                entry_pc,
            } => {
                buf.push(opcodes::CALL);
                encode_operand(&mut buf, object_id);
                buf.extend_from_slice(&(*entry_pc as u64).to_le_bytes());
            }
            Instruction::Return => buf.push(opcodes::RETURN),
            Instruction::Halt => buf.push(opcodes::HALT),
            Instruction::Read { state_id } => {
                buf.push(opcodes::READ);
                encode_operand(&mut buf, state_id);
            }
            Instruction::Write { state_id, payload } => {
                buf.push(opcodes::WRITE);
                encode_operand(&mut buf, state_id);
                buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                buf.extend_from_slice(payload);
            }
            Instruction::Effect { payload } => {
                buf.push(opcodes::EFFECT);
                buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                buf.extend_from_slice(payload);
            }
            Instruction::ObjectBirth { object_id } => {
                buf.push(opcodes::OBJECT_BIRTH);
                buf.extend_from_slice(&object_id.to_le_bytes());
            }
            Instruction::ObjectDeath { object_id } => {
                buf.push(opcodes::OBJECT_DEATH);
                encode_operand(&mut buf, object_id);
            }
            Instruction::ObjectLink { from, to, relation } => {
                buf.push(opcodes::OBJECT_LINK);
                encode_operand(&mut buf, from);
                encode_operand(&mut buf, to);
                buf.push(*relation as u8);
            }
            Instruction::Trap { service_id } => {
                buf.push(opcodes::TRAP);
                buf.push(*service_id);
            }
            Instruction::HostCall { call_id } => {
                buf.push(opcodes::HOST_CALL);
                buf.push(*call_id);
            }
            Instruction::ObjectFreeze { object_id } => {
                buf.push(opcodes::OBJECT_FREEZE);
                encode_operand(&mut buf, object_id);
            }
            Instruction::ObjectUnlink { from, to } => {
                buf.push(opcodes::OBJECT_UNLINK);
                encode_operand(&mut buf, from);
                encode_operand(&mut buf, to);
            }
            Instruction::CapabilityGrant {
                holder,
                permission,
                resource,
            } => {
                buf.push(opcodes::CAPABILITY_GRANT);
                encode_operand(&mut buf, holder);
                let perm_bytes = permission.as_bytes();
                buf.extend_from_slice(&(perm_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(perm_bytes);
                encode_operand(&mut buf, resource);
            }
            Instruction::Savepoint { name } => {
                buf.push(opcodes::SAVEPOINT);
                let name_bytes = name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(name_bytes);
            }
            Instruction::RollbackTo { name } => {
                buf.push(opcodes::ROLLBACK_TO);
                let name_bytes = name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(name_bytes);
            }
        }
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), VeritasError> {
        if bytes.is_empty() {
            return Err(VeritasError::EngineError("EOF".into()));
        }
        let opcode = bytes[0];
        let mut pos = 1;
        macro_rules! check {
            ($need:expr) => {
                if pos + $need > bytes.len() {
                    return Err(VeritasError::EngineError(format!(
                        "EOF for 0x{:02X}",
                        opcode
                    )));
                }
            };
        }

        Ok((
            match opcode {
                opcodes::NOP => Instruction::Nop,
                opcodes::LOAD_CONST => {
                    check!(9);
                    let reg = bytes[pos];
                    let val = u64::from_le_bytes(bytes[pos + 1..pos + 9].try_into().unwrap());
                    pos += 9;
                    Instruction::LoadConst { reg, val }
                }
                opcodes::ADD => {
                    check!(3);
                    let (dst, s1, s2) = (bytes[pos], bytes[pos + 1], bytes[pos + 2]);
                    pos += 3;
                    Instruction::Add {
                        dst,
                        src1: s1,
                        src2: s2,
                    }
                }
                opcodes::SUB => {
                    check!(3);
                    let (dst, s1, s2) = (bytes[pos], bytes[pos + 1], bytes[pos + 2]);
                    pos += 3;
                    Instruction::Sub {
                        dst,
                        src1: s1,
                        src2: s2,
                    }
                }
                opcodes::CMP => {
                    check!(2);
                    let (s1, s2) = (bytes[pos], bytes[pos + 1]);
                    pos += 2;
                    Instruction::Cmp { src1: s1, src2: s2 }
                }
                opcodes::JMP => {
                    check!(8);
                    let t = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap()) as usize;
                    pos += 8;
                    Instruction::Jmp { target: t }
                }
                opcodes::JZ => {
                    check!(8);
                    let t = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap()) as usize;
                    pos += 8;
                    Instruction::Jz { target: t }
                }
                opcodes::JNZ => {
                    check!(8);
                    let t = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap()) as usize;
                    pos += 8;
                    Instruction::Jnz { target: t }
                }
                opcodes::CALL => {
                    let object_id = decode_operand(bytes, &mut pos)?;
                    check!(8);
                    let entry_pc =
                        u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap()) as usize;
                    pos += 8;
                    Instruction::Call {
                        object_id,
                        entry_pc,
                    }
                }
                opcodes::RETURN => Instruction::Return,
                opcodes::JN => {
                    check!(8);
                    let t = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap()) as usize;
                    pos += 8;
                    Instruction::Jn { target: t }
                }
                opcodes::LOAD_STATE_U64 => {
                    check!(9);
                    let reg = bytes[pos];
                    let sid = u64::from_le_bytes(bytes[pos + 1..pos + 9].try_into().unwrap());
                    pos += 9;
                    Instruction::LoadStateU64 { reg, state_id: sid }
                }
                opcodes::LOAD_STATE_BYTES => {
                    check!(9);
                    let reg = bytes[pos];
                    let sid = u64::from_le_bytes(bytes[pos + 1..pos + 9].try_into().unwrap());
                    pos += 9;
                    Instruction::LoadStateBytes { reg, state_id: sid }
                }
                opcodes::WRITE_REGISTER => {
                    check!(9);
                    let sid = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
                    let reg = bytes[pos + 8];
                    pos += 9;
                    Instruction::WriteRegister { state_id: sid, reg }
                }
                opcodes::COMMIT => Instruction::Commit,
                opcodes::ABORT => Instruction::Abort {
                    reason: crate::types::AbortReason::WriteConflict,
                },
                opcodes::HALT => Instruction::Halt,
                opcodes::READ => {
                    let state_id = decode_operand(bytes, &mut pos)?;
                    Instruction::Read { state_id }
                }
                opcodes::WRITE => {
                    let state_id = decode_operand(bytes, &mut pos)?;
                    check!(4);
                    let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    check!(len);
                    let payload = bytes[pos..pos + len].to_vec();
                    pos += len;
                    Instruction::Write { state_id, payload }
                }
                opcodes::EFFECT => {
                    check!(4);
                    let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    check!(len);
                    let payload = bytes[pos..pos + len].to_vec();
                    pos += len;
                    Instruction::Effect { payload }
                }
                opcodes::OBJECT_BIRTH => {
                    check!(8);
                    let oid = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    Instruction::ObjectBirth { object_id: oid }
                }
                opcodes::OBJECT_DEATH => {
                    let object_id = decode_operand(bytes, &mut pos)?;
                    Instruction::ObjectDeath { object_id }
                }
                opcodes::OBJECT_LINK => {
                    let from = decode_operand(bytes, &mut pos)?;
                    let to = decode_operand(bytes, &mut pos)?;
                    check!(1);
                    let rel = bytes[pos];
                    pos += 1;
                    let link_type = match rel {
                        0 => crate::types::LinkType::DependsOn,
                        1 => crate::types::LinkType::Owns,
                        2 => crate::types::LinkType::References,
                        _ => {
                            return Err(VeritasError::EngineError(format!(
                                "Invalid LinkType: {}",
                                rel
                            )))
                        }
                    };
                    Instruction::ObjectLink {
                        from,
                        to,
                        relation: link_type,
                    }
                }
                opcodes::TRAP => {
                    check!(1);
                    let service_id = bytes[pos];
                    pos += 1;
                    Instruction::Trap { service_id }
                }
                opcodes::HOST_CALL => {
                    check!(1);
                    let call_id = bytes[pos];
                    pos += 1;
                    Instruction::HostCall { call_id }
                }
                opcodes::OBJECT_FREEZE => {
                    let object_id = decode_operand(bytes, &mut pos)?;
                    Instruction::ObjectFreeze { object_id }
                }
                opcodes::OBJECT_UNLINK => {
                    let from = decode_operand(bytes, &mut pos)?;
                    let to = decode_operand(bytes, &mut pos)?;
                    Instruction::ObjectUnlink { from, to }
                }
                opcodes::CAPABILITY_GRANT => {
                    let holder = decode_operand(bytes, &mut pos)?;
                    check!(4);
                    let plen = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    check!(plen);
                    let perm =
                        String::from_utf8(bytes[pos..pos + plen].to_vec()).map_err(|_| {
                            VeritasError::EngineError("Invalid UTF-8 in CapabilityGrant".into())
                        })?;
                    pos += plen;
                    let resource = decode_operand(bytes, &mut pos)?;
                    Instruction::CapabilityGrant {
                        holder,
                        permission: perm,
                        resource,
                    }
                }
                opcodes::SAVEPOINT => {
                    check!(4);
                    let nlen = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    check!(nlen);
                    let name =
                        String::from_utf8(bytes[pos..pos + nlen].to_vec()).map_err(|_| {
                            VeritasError::EngineError("Invalid UTF-8 in Savepoint".into())
                        })?;
                    pos += nlen;
                    Instruction::Savepoint { name }
                }
                opcodes::ROLLBACK_TO => {
                    check!(4);
                    let nlen = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4;
                    check!(nlen);
                    let name =
                        String::from_utf8(bytes[pos..pos + nlen].to_vec()).map_err(|_| {
                            VeritasError::EngineError("Invalid UTF-8 in RollbackTo".into())
                        })?;
                    pos += nlen;
                    Instruction::RollbackTo { name }
                }
                _ => {
                    return Err(VeritasError::EngineError(format!(
                        "Unknown opcode: 0x{:02X}",
                        opcode
                    )))
                }
            },
            pos,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p15_1_codec_roundtrip() {
        let insts = vec![
            Instruction::LoadConst { reg: 0, val: 55 },
            Instruction::Add {
                dst: 2,
                src1: 0,
                src2: 1,
            },
            Instruction::Cmp { src1: 1, src2: 3 },
            Instruction::Jnz { target: 4 },
            Instruction::WriteRegister {
                state_id: 100,
                reg: 2,
            },
            Instruction::Commit,
            Instruction::Halt,
        ];
        for orig in insts {
            let enc = orig.encode().unwrap();
            let (dec, n) = Instruction::decode(&enc).unwrap();
            assert_eq!(n, enc.len());
            assert_eq!(orig, dec);
        }
    }
}
