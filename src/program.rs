use crate::instruction::Instruction;

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub instructions: Vec<Instruction>,
}

impl Program {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
        }
    }

    pub fn push(mut self, inst: Instruction) -> Self {
        self.instructions.push(inst);
        self
    }

    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn get(&self, index: usize) -> Option<&Instruction> {
        self.instructions.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for inst in &self.instructions {
            for b in inst.encode().unwrap_or_default() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

// ===== P15.2: ProgramImage 二进制镜像格式 =====

pub const VERI_MAGIC: &[u8; 4] = b"VERI";
pub const CURRENT_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramImage {
    pub version: u16,
    pub entry_point: u32,
    pub instructions: Vec<crate::instruction::Instruction>,
}

impl ProgramImage {
    pub fn new(instructions: Vec<crate::instruction::Instruction>) -> Self {
        Self {
            version: CURRENT_VERSION,
            entry_point: 0,
            instructions,
        }
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for inst in &self.instructions {
            for b in inst.encode().unwrap_or_default() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    fn checksum(data: &[u8]) -> u32 {
        let mut h = crc32fast::Hasher::new();
        h.update(data);
        h.finalize()
    }

    pub fn encode(&self) -> Result<Vec<u8>, crate::types::VeritasError> {
        let mut body = Vec::new();
        for inst in &self.instructions {
            body.extend_from_slice(&inst.encode()?);
        }

        let mut prefix = Vec::with_capacity(14);
        prefix.extend_from_slice(VERI_MAGIC);
        prefix.extend_from_slice(&self.version.to_le_bytes());
        prefix.extend_from_slice(&self.entry_point.to_le_bytes());
        prefix.extend_from_slice(&(self.instructions.len() as u32).to_le_bytes());

        let mut check_data = Vec::new();
        check_data.extend_from_slice(&prefix);
        check_data.extend_from_slice(&body);
        let cs = Self::checksum(&check_data);

        let mut out = Vec::new();
        out.extend_from_slice(&prefix);
        out.extend_from_slice(&cs.to_le_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, crate::types::VeritasError> {
        use crate::types::VeritasError;
        if bytes.len() < 18 {
            return Err(VeritasError::EngineError("Image too short".into()));
        }
        if &bytes[0..4] != VERI_MAGIC {
            return Err(VeritasError::EngineError("Bad magic".into()));
        }
        let version = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        if version > CURRENT_VERSION {
            return Err(VeritasError::EngineError(format!(
                "Unsupported version {}",
                version
            )));
        }
        let entry_point = u32::from_le_bytes(bytes[6..10].try_into().unwrap());
        let inst_count = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let expected_cs = u32::from_le_bytes(bytes[14..18].try_into().unwrap());

        let mut check_data = Vec::new();
        check_data.extend_from_slice(&bytes[0..14]);
        check_data.extend_from_slice(&bytes[18..]);
        let actual_cs = Self::checksum(&check_data);
        if expected_cs != actual_cs {
            return Err(VeritasError::EngineError(format!(
                "Checksum mismatch: expected {:08X} got {:08X}",
                expected_cs, actual_cs
            )));
        }

        let mut offset = 18;
        let mut instructions = Vec::with_capacity(inst_count);
        for _ in 0..inst_count {
            if offset >= bytes.len() {
                return Err(VeritasError::EngineError(
                    "EOF decoding instructions".into(),
                ));
            }
            let slice = &bytes[offset..];
            let (inst, consumed) = crate::instruction::Instruction::decode(slice)?;
            instructions.push(inst);
            offset += consumed;
        }
        Ok(Self {
            version,
            entry_point,
            instructions,
        })
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::instruction::Instruction;

    #[test]
    fn test_p15_2_program_image_roundtrip() {
        let insts = vec![
            Instruction::LoadConst { reg: 0, val: 100 },
            Instruction::LoadConst { reg: 1, val: 200 },
            Instruction::Add {
                dst: 2,
                src1: 0,
                src2: 1,
            },
            Instruction::Trap { service_id: 5 },
            Instruction::Halt,
        ];
        let image = ProgramImage::new(insts);
        let bytes = image.encode().unwrap();
        let decoded = ProgramImage::decode(&bytes).unwrap();
        assert_eq!(image, decoded);
    }

    #[test]
    fn test_p15_2_checksum_tamper_detection() {
        let insts = vec![
            Instruction::LoadConst { reg: 0, val: 42 },
            Instruction::Halt,
        ];
        let image = ProgramImage::new(insts);
        let mut bytes = image.encode().unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(ProgramImage::decode(&bytes).is_err());
    }
}
