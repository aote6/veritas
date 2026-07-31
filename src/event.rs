use crate::instruction::Instruction;
use crate::types::StateId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionEvent {
    InstructionStart { pc: usize, inst: Instruction },
    InstructionEnd { pc: usize, regs_after: [u64; 8] },
    StateRead { state_id: StateId },
    StateWrite { state_id: StateId, len: usize },
    CapabilityCheck { cap_id: u64, passed: bool },
    Commit,
    Abort,
    Halted,
    Trapped { reason: String },
}

impl ExecutionEvent {
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        match self {
            ExecutionEvent::InstructionStart { pc, inst } => {
                h ^= *pc as u64;
                h = h.wrapping_mul(0x100000001b3);
                h ^= inst.opcode() as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            ExecutionEvent::StateWrite { state_id, len } => {
                h ^= *state_id;
                h = h.wrapping_mul(0x100000001b3);
                h ^= *len as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            ExecutionEvent::Halted => { h ^= 0xFF; }
            _ => { h ^= 0x01; }
        }
        h
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventRecorder {
    pub events: Vec<ExecutionEvent>,
}

impl EventRecorder {
    pub fn new() -> Self { Self::default() }
    pub fn push(&mut self, e: ExecutionEvent) { self.events.push(e); }
    pub fn event_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for e in &self.events {
            h ^= e.hash();
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
    fn test_event_hash_deterministic() {
        let mut r1 = EventRecorder::new();
        r1.push(ExecutionEvent::InstructionStart { pc: 0, inst: Instruction::Nop });
        r1.push(ExecutionEvent::Halted);

        let mut r2 = EventRecorder::new();
        r2.push(ExecutionEvent::InstructionStart { pc: 0, inst: Instruction::Nop });
        r2.push(ExecutionEvent::Halted);

        assert_eq!(r1.event_hash(), r2.event_hash());
    }
}
