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
                buf.push(*dst); buf.push(*src1); buf.push(*src2);
            }
            Instruction::Sub { dst, src1, src2 } => {
                buf.push(opcodes::SUB);
                buf.push(*dst); buf.push(*src1); buf.push(*src2);
            }
            Instruction::Cmp { src1, src2 } => {
                buf.push(opcodes::CMP);
                buf.push(*src1); buf.push(*src2);
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
            Instruction::Halt => buf.push(opcodes::HALT),
            _ => return Err(VeritasError::EngineError(
                "Kernel instruction not encodable yet".into()
            )),
        }
        Ok(buf)
    }

    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), VeritasError> {
        if bytes.is_empty() {
            return Err(VeritasError::EngineError("EOF".into()));
        }
        let opcode = bytes[0];
        let mut pos = 1;
        let check = |need: usize| -> Result<(), VeritasError> {
            if pos + need > bytes.len() {
                Err(VeritasError::EngineError(format!("EOF for 0x{:02X}", opcode)))
            } else { Ok(()) }
        };

        Ok((match opcode {
            opcodes::NOP => Instruction::Nop,
            opcodes::LOAD_CONST => {
                check(9)?;
                let reg = bytes[pos];
                let val = u64::from_le_bytes(bytes[pos+1..pos+9].try_into().unwrap());
                pos += 9;
                Instruction::LoadConst { reg, val }
            }
            opcodes::ADD => {
                check(3)?;
                let (dst, s1, s2) = (bytes[pos], bytes[pos+1], bytes[pos+2]);
                pos += 3;
                Instruction::Add { dst, src1: s1, src2: s2 }
            }
            opcodes::SUB => {
                check(3)?;
                let (dst, s1, s2) = (bytes[pos], bytes[pos+1], bytes[pos+2]);
                pos += 3;
                Instruction::Sub { dst, src1: s1, src2: s2 }
            }
            opcodes::CMP => {
                check(2)?;
                let (s1, s2) = (bytes[pos], bytes[pos+1]);
                pos += 2;
                Instruction::Cmp { src1: s1, src2: s2 }
            }
            opcodes::JMP => {
                check(8)?;
                let t = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap()) as usize;
                pos += 8;
                Instruction::Jmp { target: t }
            }
            opcodes::JZ => {
                check(8)?;
                let t = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap()) as usize;
                pos += 8;
                Instruction::Jz { target: t }
            }
            opcodes::JNZ => {
                check(8)?;
                let t = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap()) as usize;
                pos += 8;
                Instruction::Jnz { target: t }
            }
            opcodes::JN => {
                check(8)?;
                let t = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap()) as usize;
                pos += 8;
                Instruction::Jn { target: t }
            }
            opcodes::LOAD_STATE_U64 => {
                check(9)?;
                let reg = bytes[pos];
                let sid = u64::from_le_bytes(bytes[pos+1..pos+9].try_into().unwrap());
                pos += 9;
                Instruction::LoadStateU64 { reg, state_id: sid }
            }
            opcodes::LOAD_STATE_BYTES => {
                check(9)?;
                let reg = bytes[pos];
                let sid = u64::from_le_bytes(bytes[pos+1..pos+9].try_into().unwrap());
                pos += 9;
                Instruction::LoadStateBytes { reg, state_id: sid }
            }
            opcodes::WRITE_REGISTER => {
                check(9)?;
                let sid = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap());
                let reg = bytes[pos+8];
                pos += 9;
                Instruction::WriteRegister { state_id: sid, reg }
            }
            opcodes::COMMIT => Instruction::Commit,
            opcodes::ABORT => Instruction::Abort { reason: crate::types::AbortReason::WriteConflict },
            opcodes::HALT => Instruction::Halt,
            _ => return Err(VeritasError::EngineError(format!("Unknown opcode: 0x{:02X}", opcode))),
        }, pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p15_1_codec_roundtrip() {
        let insts = vec![
            Instruction::LoadConst { reg: 0, val: 55 },
            Instruction::Add { dst: 2, src1: 0, src2: 1 },
            Instruction::Cmp { src1: 1, src2: 3 },
            Instruction::Jnz { target: 4 },
            Instruction::WriteRegister { state_id: 100, reg: 2 },
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
