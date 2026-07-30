use crate::trace::TraceRecorder;
use crate::types::WriteSet;
use crate::receipt::ExecutionReceipt;

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub program_hash: u64,
    pub input_root: u64,
    pub trace: TraceRecorder,
    pub writes: WriteSet,
    pub instruction_count: u64,
}

impl ExecutionContext {
    pub fn new(program_hash: u64, input_root: u64) -> Self {
        Self {
            program_hash,
            input_root,
            trace: TraceRecorder::new(),
            writes: WriteSet { changes: vec![] },
            instruction_count: 0,
        }
    }

    pub fn finalize(&self, output_root: u64) -> ExecutionReceipt {
        ExecutionReceipt {
            program_hash: self.program_hash,
            input_root: self.input_root,
            output_root,
            trace_hash: self.trace.trace_hash(),
            write_set_hash: self.writes.hash(),
            instruction_count: self.instruction_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_finalize_produces_receipt() {
        let ctx = ExecutionContext::new(42, 100);
        let receipt = ctx.finalize(200);
        assert_eq!(receipt.program_hash, 42);
        assert_eq!(receipt.input_root, 100);
        assert_eq!(receipt.output_root, 200);
        assert!(receipt.verify());
    }
}
