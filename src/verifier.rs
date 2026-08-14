use crate::instruction::Instruction;
use crate::program::Program;
use crate::types::VeritasError;

pub struct Verifier;

impl Verifier {
    pub fn verify(program: &Program) -> Result<(), VeritasError> {
        if program.is_empty() {
            return Err(VeritasError::EngineError(
                "Empty program cannot be executed".into(),
            ));
        }

        let mut has_terminal = false;
        for (idx, inst) in program.instructions.iter().enumerate() {
            if has_terminal {
                return Err(VeritasError::EngineError(format!(
                    "Instruction after terminal at index {}",
                    idx
                )));
            }

            match inst {
                Instruction::Commit | Instruction::Abort { .. } => {
                    has_terminal = true;
                }
                Instruction::Write { payload, .. } if payload.is_empty() => {
                    return Err(VeritasError::EngineError(
                        "Empty payload in Write instruction".into(),
                    ));
                }
                _ => {}
            }
        }

        Ok(())
    }
}
