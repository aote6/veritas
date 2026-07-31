use crate::execution::ExecutionContext;
use crate::event::EventRecorder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub program_hash: u64,
    pub input_root: u64,
    pub output_root: u64,
    pub trace_hash: u64,
    pub write_set_hash: u64,
    pub event_hash: u64,
    pub instruction_count: u64,
    pub reads: u64,
    pub writes: u64,
    pub capability_hash: u64,
}

impl ExecutionReceipt {
    pub fn verify(&self) -> bool {
        self.program_hash != 0
            && self.trace_hash != 0
            && self.event_hash != 0
            && self.capability_hash != 0
    }

    pub fn matches(&self, other: &ExecutionReceipt) -> bool {
        self.program_hash == other.program_hash
            && self.input_root == other.input_root
            && self.output_root == other.output_root
            && self.trace_hash == other.trace_hash
            && self.write_set_hash == other.write_set_hash
            && self.event_hash == other.event_hash
            && self.capability_hash == other.capability_hash
            && self.instruction_count == other.instruction_count
    }
}

pub struct ReceiptBuilder;

impl ReceiptBuilder {
    fn hash_caps(ids: &[u64]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &id in ids {
            h ^= id;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
    pub fn build(ctx: &ExecutionContext, output_root: u64) -> ExecutionReceipt {
        ExecutionReceipt {
            program_hash: ctx.program_hash,
            input_root: ctx.input_root,
            output_root,
            trace_hash: ctx.trace.trace_hash(),
            write_set_hash: ctx.writes.hash(),
            event_hash: ctx.events.event_hash(),
            instruction_count: ctx.instruction_count,
            reads: ctx.stats.reads,
            writes: ctx.stats.writes,
            capability_hash: Self::hash_caps(&ctx.capability_ids),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receipt_verify_rejects_zero() {
        let r = ExecutionReceipt {
            program_hash: 0, input_root: 1, output_root: 2,
            trace_hash: 3, write_set_hash: 4, event_hash: 0, capability_hash: 0,
            instruction_count: 1, reads: 0, writes: 0,
        };
        assert!(!r.verify());
    }

    #[test]
    fn test_receipt_matches_detects_difference() {
        let r1 = ExecutionReceipt {
            program_hash: 1, input_root: 2, output_root: 3,
            trace_hash: 4, write_set_hash: 5, event_hash: 6, capability_hash: 0,
            instruction_count: 1, reads: 0, writes: 0,
        };
        let r2 = ExecutionReceipt {
            program_hash: 9, ..r1.clone()
        };
        assert!(!r1.matches(&r2));
    }
}
