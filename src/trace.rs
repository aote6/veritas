use crate::instruction::Instruction;
use crate::types::StateId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionTrace {
    pub pc: usize,
    pub opcode: u8,
    pub instruction: Instruction,
    pub registers_before: [u64; 8],
    pub registers_after: [u64; 8],
    pub state_reads: Vec<StateId>,
    pub state_writes: Vec<StateId>,
}

impl InstructionTrace {
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.pc as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.opcode as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &r in &self.registers_after {
            h ^= r;
            h = h.wrapping_mul(0x100000001b3);
        }
        for &s in &self.state_writes {
            h ^= s;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

#[derive(Debug, Clone, Default)]
pub struct TraceRecorder {
    pub traces: Vec<InstructionTrace>,
}

impl TraceRecorder {
    pub fn new() -> Self { Self::default() }
    pub fn push(&mut self, t: InstructionTrace) { self.traces.push(t); }
    pub fn trace_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for t in &self.traces {
            h ^= t.hash();
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::Instruction;

    #[test]
    fn test_trace_hash_deterministic() {
        let t1 = InstructionTrace {
            pc: 0, opcode: 0x01,
            instruction: Instruction::LoadConst { reg: 0, val: 42 },
            registers_before: [0; 8],
            registers_after: [42, 0, 0, 0, 0, 0, 0, 0],
            state_reads: vec![],
            state_writes: vec![],
        };
        let t2 = t1.clone();
        assert_eq!(t1.hash(), t2.hash());

        let mut r1 = TraceRecorder::new();
        r1.push(t1);
        let mut r2 = TraceRecorder::new();
        r2.push(t2);
        assert_eq!(r1.trace_hash(), r2.trace_hash());
    }
}
