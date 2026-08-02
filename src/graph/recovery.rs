use crate::graph::journal::GraphJournal;
use crate::graph::replay::{ReplayEngine, ReplayError, ReplayMode};
use crate::graph::store::GraphStore;

/// Graph 崩溃恢复器：纯函数式/无状态恢复入口
/// 负责在系统掉电或崩溃重启时，从 WAL Journal 镜像还原绝对一致的 GraphStore 最终状态
pub struct GraphRecovery;

impl GraphRecovery {
    /// 从给定的 GraphJournal 重建 GraphStore 最终状态（崩溃丢弃未提交事务）
    pub fn recover(journal: &GraphJournal) -> Result<GraphStore, ReplayError> {
        let mut store = GraphStore::new();
        ReplayEngine::replay_with_mode(journal, &mut store, ReplayMode::Recovery)?;
        Ok(store)
    }

    /// 恢复并校验恢复后的存储是否满足幂等性与一致性约束
    pub fn recover_and_verify(journal: &GraphJournal) -> Result<GraphStore, ReplayError> {
        let store1 = Self::recover(journal)?;
        let store2 = Self::recover(journal)?;

        if store1 != store2 {
            panic!("graph recovery not deterministic");
        }

        Ok(store1)
    }
}
