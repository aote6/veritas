use crate::state_memory::StateMemory;
use crate::history::ExecutionHistory;
use crate::checkpoint::Checkpoint;
use crate::types::VeritasError;

pub struct ReplayEngine;

impl ReplayEngine {
    pub fn replay(
        from: &Checkpoint,
        history: &ExecutionHistory,
    ) -> Result<Checkpoint, VeritasError> {
        let mut state = StateMemory::new();
        state.restore(&from.snapshot);

        for entry in history.entries() {
            let rec = &entry.record;
            if rec.before_root != state.root_hash() {
                return Err(VeritasError::EngineError(format!(
                    "Replay chain broken at v{}: expected before_root {:016X}, got {:016X}",
                    entry.version, rec.before_root, state.root_hash()
                )));
            }
            // 临时占位：ReplayRecord目前不记录object_id，统一归属内核Object(0)。
            // 待ReplayRecord/WAL扩展支持Object寻址后应移除此转换。
            for (id, val) in &rec.writes {
                state.write(crate::types::Address::new(0, *id), val.clone());
            }
            if rec.after_root != state.root_hash() {
                return Err(VeritasError::EngineError(format!(
                    "Replay mismatch at v{}: expected after_root {:016X}, got {:016X}",
                    entry.version, rec.after_root, state.root_hash()
                )));
            }
        }

        Ok(Checkpoint::new(state.snapshot()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::ReplayRecord;

    #[test]
    fn test_replay_deterministic() {
        let mut state = StateMemory::new();
        state.write(crate::types::Address::new(0, 1), vec![10]);
        let cp1 = Checkpoint::new(state.snapshot());

        let mut history = ExecutionHistory::new();
        let before = state.root_hash();
        state.write(crate::types::Address::new(0, 2), vec![20]);
        let after = state.root_hash();
        history.push(ReplayRecord::new(1, None, 0, vec![(2, vec![20])], before, after));

        let result = ReplayEngine::replay(&cp1, &history).unwrap();
        assert_eq!(result.state_root, after);
        assert!(result.verify());
    }

    #[test]
    fn test_replay_chain_break_detected() {
        let mut state = StateMemory::new();
        state.write(crate::types::Address::new(0, 1), vec![1]);
        let cp = Checkpoint::new(state.snapshot());

        let mut history = ExecutionHistory::new();
        // before_root 故意设为错误值
        history.push(ReplayRecord::new(1, None, 0, vec![(2, vec![2])], 0xDEADBEEF, 0));

        assert!(ReplayEngine::replay(&cp, &history).is_err());
    }
}
