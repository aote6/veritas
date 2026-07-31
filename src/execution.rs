use crate::trace::{TraceRecorder, InstructionTrace};
use crate::instruction::Instruction;
use crate::types::{WriteSet, StateId};
use crate::receipt::ExecutionReceipt;

#[derive(Debug, Clone, Default)]
pub struct ExecutionStatistics {
    pub instructions: u64,
    pub reads: u64,
    pub writes: u64,
}

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
    pub stats: ExecutionStatistics,
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
            stats: ExecutionStatistics::default(),
            pending: None,
        }
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

    pub fn record_instruction(&mut self, trace: InstructionTrace) {
        self.trace.push(trace);
        self.instruction_count += 1;
        self.stats.instructions += 1;
    }

    pub fn record_write(&mut self, state_id: StateId, value: Vec<u8>) {
        self.writes.push(state_id, value);
        self.stats.writes += 1;
    }

    pub fn record_read(&mut self, _state_id: StateId) {
        self.stats.reads += 1;
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

    #[test]
    fn test_stats_count_instructions_and_writes() {
        let mut ctx = ExecutionContext::new(1, 0);
        ctx.record_instruction(InstructionTrace {
            pc: 0, opcode: 1,
            instruction: Instruction::Nop,
            registers_before: [0; 8],
            registers_after: [0; 8],
            state_reads: vec![],
            state_writes: vec![],
        });
        ctx.record_write(1, vec![1, 2, 3]);
        ctx.record_read(2);
        assert_eq!(ctx.stats.instructions, 1);
        assert_eq!(ctx.stats.writes, 1);
        assert_eq!(ctx.stats.reads, 1);
    }
}
