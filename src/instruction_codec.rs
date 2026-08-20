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
    pub const HALT: u8 = 0xFF;

    // Machine I/O and control (not Kernel service opcodes)
    pub const READ: u8 = 0x30;
    pub const WRITE: u8 = 0x31;
    pub const HOST_CALL: u8 = 0x40;
    pub const TRAP: u8 = 0x41;
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

fn decode_operand(bytes: &[u8], pos: &mut usize) -> Result<crate::instruction::Operand, VeritasError> {
    if *pos >= bytes.len() {
        return Err(VeritasError::EngineError("EOF operand tag".into()));
    }
    let tag = bytes[*pos];
    *pos += 1;
    match tag {
        0 => {
            if *pos + 8 > bytes.len() {
                return Err(VeritasError::EngineError("EOF operand imm".into()));
            }
            let v = u64::from_le_bytes(bytes[*pos..*pos + 8].try_into().unwrap());
            *pos += 8;
            Ok(crate::instruction::Operand::Immediate(v))
        }
        1 => {
            if *pos + 8 > bytes.len() {
                return Err(VeritasError::EngineError("EOF operand reg".into()));
            }
            let r = bytes[*pos] as u8;
            *pos += 8;
            Ok(crate::instruction::Operand::Register(r))
        }
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
            Instruction::Trap { service_id } => {
                buf.push(opcodes::TRAP);
                buf.push(*service_id);
            }
            Instruction::HostCall { call_id } => {
                buf.push(opcodes::HOST_CALL);
                buf.push(*call_id);
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
            Instruction::Trap { service_id: 5 }, // Commit via TRAP
            Instruction::Halt,
        ];
        for orig in insts {
            let enc = orig.encode().unwrap();
            let (dec, n) = Instruction::decode(&enc).unwrap();
            assert_eq!(n, enc.len());
            assert_eq!(orig, dec);
        }
    }

    #[test]
    fn legacy_kernel_opcodes_rejected() {
        // Retired Kernel service opcodes must not decode
        for op in [0x23u8, 0x24, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x3B, 0x3C] {
            assert!(
                Instruction::decode(&[op]).is_err(),
                "legacy opcode 0x{:02X} should be rejected",
                op
            );
        }
    }
}
