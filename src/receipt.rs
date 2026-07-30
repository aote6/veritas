#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub program_hash: u64,
    pub input_root: u64,
    pub output_root: u64,
    pub trace_hash: u64,
    pub write_set_hash: u64,
    pub instruction_count: u64,
}

impl ExecutionReceipt {
    pub fn matches(&self, other: &ExecutionReceipt) -> bool {
        self.program_hash == other.program_hash
            && self.input_root == other.input_root
            && self.output_root == other.output_root
            && self.trace_hash == other.trace_hash
            && self.write_set_hash == other.write_set_hash
            && self.instruction_count == other.instruction_count
    }

    pub fn verify(&self) -> bool {
        self.program_hash != 0
            && self.trace_hash != 0
            && self.input_root != 0
            && self.output_root != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receipt_verify_rejects_zero() {
        let r = ExecutionReceipt {
            program_hash: 0, input_root: 1, output_root: 2,
            trace_hash: 3, write_set_hash: 4, instruction_count: 1,
        };
        assert!(!r.verify());
    }

    #[test]
    fn test_receipt_verify_passes_valid() {
        let r = ExecutionReceipt {
            program_hash: 1, input_root: 1, output_root: 2,
            trace_hash: 3, write_set_hash: 4, instruction_count: 1,
        };
        assert!(r.verify());
    }
}
