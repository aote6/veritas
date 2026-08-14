use crate::program::ProgramImage;
use crate::types::VeritasError;

pub const VMOD_MAGIC: &[u8; 4] = b"VMOD";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ModuleVersion {
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleImage {
    pub name: String,
    pub version: ModuleVersion,
    pub program_image: ProgramImage,
}

impl ModuleImage {
    pub fn new(name: &str, version: ModuleVersion, program_image: ProgramImage) -> Self {
        Self {
            name: name.into(),
            version,
            program_image,
        }
    }

    pub fn encode_file(&self) -> Result<Vec<u8>, VeritasError> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(VMOD_MAGIC);
        bytes.extend_from_slice(&self.version.major.to_le_bytes());
        bytes.extend_from_slice(&self.version.minor.to_le_bytes());
        bytes.extend_from_slice(&self.version.patch.to_le_bytes());
        let name_bytes = self.name.as_bytes();
        bytes.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name_bytes);
        let prog_bytes = self.program_image.encode()?;
        bytes.extend_from_slice(&(prog_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&prog_bytes);
        Ok(bytes)
    }

    pub fn decode_file(bytes: &[u8]) -> Result<Self, VeritasError> {
        if bytes.len() < 20 || &bytes[0..4] != VMOD_MAGIC {
            return Err(VeritasError::EngineError("Bad VMOD magic".into()));
        }
        let major = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
        let minor = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
        let patch = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let name_len = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let name = std::str::from_utf8(&bytes[14..14 + name_len])
            .map_err(|_| VeritasError::EngineError("Bad UTF-8".into()))?;
        let mut pos = 14 + name_len;
        let prog_len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let program_image = ProgramImage::decode(&bytes[pos..pos + prog_len])?;
        Ok(Self {
            name: name.into(),
            version: ModuleVersion::new(major, minor, patch),
            program_image,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::Instruction;

    #[test]
    fn test_vmod_roundtrip() {
        let prog = ProgramImage::new(vec![Instruction::Halt]);
        let module = ModuleImage::new("test", ModuleVersion::new(1, 0, 0), prog);
        let bytes = module.encode_file().unwrap();
        let decoded = ModuleImage::decode_file(&bytes).unwrap();
        assert_eq!(decoded.name, "test");
        assert_eq!(decoded.version, ModuleVersion::new(1, 0, 0));
        assert_eq!(decoded.program_image.instructions.len(), 1);
    }
}

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    Loaded,
    Verified,
    Installed,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub image: ModuleImage,
    pub status: ModuleStatus,
}

#[derive(Debug, Default)]
pub struct ModuleLoader {
    modules: HashMap<String, LoadedModule>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    pub fn load_bytes(&self, bytes: &[u8]) -> Result<ModuleImage, VeritasError> {
        ModuleImage::decode_file(bytes)
    }

    pub fn verify(&self, image: &ModuleImage) -> Result<(), VeritasError> {
        if image.name.trim().is_empty() {
            return Err(VeritasError::EngineError("Module name empty".into()));
        }
        if image.program_image.instructions.is_empty() {
            return Err(VeritasError::EngineError("Module program empty".into()));
        }
        Ok(())
    }

    pub fn install(&mut self, image: ModuleImage) -> Result<String, VeritasError> {
        self.verify(&image)?;
        let name = image.name.clone();
        self.modules.insert(
            name.clone(),
            LoadedModule {
                image,
                status: ModuleStatus::Installed,
            },
        );
        Ok(name)
    }

    pub fn load_and_install(&mut self, bytes: &[u8]) -> Result<String, VeritasError> {
        let image = self.load_bytes(bytes)?;
        self.install(image)
    }

    pub fn get_module(&self, name: &str) -> Option<&LoadedModule> {
        self.modules.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }
}

#[cfg(test)]
mod loader_tests {
    use super::*;
    use crate::instruction::Instruction;

    #[test]
    fn test_loader_lifecycle() {
        let prog = ProgramImage::new(vec![Instruction::Nop, Instruction::Halt]);
        let m = ModuleImage::new("core.sys", ModuleVersion::new(1, 0, 0), prog);
        let bytes = m.encode_file().unwrap();

        let mut loader = ModuleLoader::new();
        let decoded = loader.load_bytes(&bytes).unwrap();
        assert_eq!(decoded.name, "core.sys");
        assert!(loader.verify(&decoded).is_ok());

        let name = loader.install(decoded).unwrap();
        assert_eq!(name, "core.sys");
        assert!(loader.contains("core.sys"));
        assert_eq!(
            loader.get_module("core.sys").unwrap().status,
            ModuleStatus::Installed
        );
    }

    #[test]
    fn test_verify_empty_program_rejected() {
        let prog = ProgramImage::new(vec![]);
        let m = ModuleImage::new("empty", ModuleVersion::new(0, 0, 1), prog);
        let loader = ModuleLoader::new();
        assert!(loader.verify(&m).is_err());
    }
}
