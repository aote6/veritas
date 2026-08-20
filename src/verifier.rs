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

        for inst in program.instructions.iter() {
            match inst {
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
