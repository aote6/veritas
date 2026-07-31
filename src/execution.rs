use crate::trace::{TraceRecorder, InstructionTrace};
use crate::instruction::Instruction;
use crate::types::WriteSet;
use crate::receipt::ExecutionReceipt;

#[derive(Debug, Clone)]
pub struct PendingInstruction {
    pub pc: usize,
    pub regs_before: [u64; 8],
    pub instruction: Instruction,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub program_hash: u64,
    pub input_root: u64,
    pub trace: TraceRecorder,
    pub writes: WriteSet,
    pub instruction_count: u64,
    pending: Option<PendingInstruction>,
}

impl ExecutionContext {
    pub fn new(program_hash: u64, input_root: u64) -> Self {
        Self {
            program_hash,
            input_root,
            trace: TraceRecorder::new(),
            writes: WriteSet { changes: vec![] },
            instruction_count: 0,
            pending: None,
        }
    }

    pub fn record_instruction(&mut self, trace: crate::trace::InstructionTrace) {
        self.trace.push(trace);
        self.instruction_count += 1;
    }

    pub fn record_write(&mut self, state_id: crate::types::StateId, value: Vec<u8>) {
        self.writes.push(state_id, value);
    }

    pub fn begin_instruction(&mut self, pc: usize, regs: [u64; 8], inst: Instruction) {
        self.pending = Some(PendingInstruction { pc, regs_before: regs, instruction: inst });
    }

    pub fn finish_instruction(&mut self, regs_after: [u64; 8]) {
        if let Some(pending) = self.pending.take() {
            self.record_instruction(InstructionTrace {
                pc: pending.pc,
                opcode: pending.instruction.opcode() as u8,
                instruction: pending.instruction,
                registers_before: pending.regs_before,
                registers_after: regs_after,
                state_reads: vec![],
                state_writes: vec![],
            });
        }
    }

    pub fn record_read(&mut self, _state_id: crate::types::StateId) {
        // 预留: 记录状态读取
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
