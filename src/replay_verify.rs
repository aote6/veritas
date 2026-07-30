use crate::receipt::ExecutionReceipt;
use crate::types::VeritasError;

pub struct ReplayVerifier;

impl ReplayVerifier {
    pub fn verify(expected: &ExecutionReceipt, actual: &ExecutionReceipt) -> Result<(), VeritasError> {
        if expected.matches(actual) {
            Ok(())
        } else {
            Err(VeritasError::DeterminismViolation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_passes_on_match() {
        let r = ExecutionReceipt {
            program_hash: 1, input_root: 2, output_root: 3,
            trace_hash: 4, write_set_hash: 5, instruction_count: 1,
        };
        assert!(ReplayVerifier::verify(&r, &r).is_ok());
    }

    #[test]
    fn test_verify_detects_difference() {
        let r1 = ExecutionReceipt {
            program_hash: 1, input_root: 2, output_root: 3,
            trace_hash: 4, write_set_hash: 5, instruction_count: 1,
        };
        let r2 = ExecutionReceipt {
            program_hash: 9, input_root: 2, output_root: 3,
            trace_hash: 4, write_set_hash: 5, instruction_count: 1,
        };
        assert!(ReplayVerifier::verify(&r1, &r2).is_err());
    }
}
