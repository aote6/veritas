// Veritas Kernel - Transaction 模块
// Phase 6: 事务上下文独立管理

use crate::types::{TxId, Version, ReadSet, WriteSet};
use crate::effect::EffectQueue;

/// 事务上下文
#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub tx_id: TxId,
    pub snapshot_version: Version,
    pub read_set: ReadSet,
    pub write_set: WriteSet,
    pub effect_queue: EffectQueue,
    pub aborted: bool,
}

impl TransactionContext {
    /// 创建新事务上下文
    pub fn new(tx_id: TxId, snapshot_version: Version) -> Self {
        TransactionContext {
            tx_id,
            snapshot_version,
            read_set: ReadSet::default(),
            write_set: WriteSet::default(),
            effect_queue: EffectQueue::default(),
            aborted: false,
        }
    }

    /// 标记为已中止
    pub fn set_aborted(&mut self) {
        self.aborted = true;
    }

    /// 检查是否已中止
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// 获取事务ID
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    /// 获取快照版本
    pub fn snapshot_version(&self) -> Version {
        self.snapshot_version
    }

    /// 清空事务上下文（用于复用）
    pub fn clear(&mut self) {
        self.read_set.states.clear();
        self.write_set.state_changes.clear();
        self.effect_queue = EffectQueue::default();
        self.aborted = false;
    }
}
