use crate::types::TxId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 事务物理状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    Active,
    Committed,
    Aborted,
}

/// PCB：事务控制块
pub struct ActiveTransaction {
    pub id: TxId,
    pub state: TransactionState,
}

impl ActiveTransaction {
    pub fn new(id: TxId) -> Self {
        Self {
            id,
            state: TransactionState::Active,
        }
    }
}

/// 事务管理器：独占 TxId 分配 + 进程表
pub struct TransactionManager {
    next_tx_id: AtomicU64,
    tx_table: Mutex<HashMap<TxId, ActiveTransaction>>,
}

impl TransactionManager {
    /// 获取当前 atomic 计数器的值（下一笔事务将分配的 TxId 水位）
    pub fn current_tx_id(&self) -> u64 {
        self.next_tx_id.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn new() -> Self {
        Self::with_start_id(1)
    }

    pub fn with_start_id(start: u64) -> Self {
        Self {
            next_tx_id: AtomicU64::new(start),
            tx_table: Mutex::new(HashMap::new()),
        }
    }

    /// 唯一事务入口：分配 TxId → 建 PCB → 返回 TxId
    pub fn begin(&self) -> TxId {
        let tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);
        let active = ActiveTransaction::new(tx_id);
        let mut table = self.tx_table.lock().unwrap();
        table.insert(tx_id, active);
        tx_id
    }

    /// 标记事务已提交
    pub fn mark_committed(&self, tx_id: TxId) {
        let mut table = self.tx_table.lock().unwrap();
        if let Some(tx) = table.get_mut(&tx_id) {
            tx.state = TransactionState::Committed;
        }
    }

    /// 标记事务已中止（或被击毙）
    pub fn mark_aborted(&self, tx_id: TxId) {
        let mut table = self.tx_table.lock().unwrap();
        if let Some(tx) = table.get_mut(&tx_id) {
            tx.state = TransactionState::Aborted;
        }
    }

    /// 查询事务是否存活
    pub fn is_active(&self, tx_id: TxId) -> bool {
        let table = self.tx_table.lock().unwrap();
        table
            .get(&tx_id)
            .map_or(false, |tx| tx.state == TransactionState::Active)
    }

    /// 查询事务是否已被击毙
    pub fn is_aborted(&self, tx_id: TxId) -> bool {
        let table = self.tx_table.lock().unwrap();
        table
            .get(&tx_id)
            .map_or(false, |tx| tx.state == TransactionState::Aborted)
    }

    /// 从事务表中移除已结束的事务（防止 PCB 泄漏）
    pub fn remove(&self, tx_id: TxId) {
        let mut table = self.tx_table.lock().unwrap();
        table.remove(&tx_id);
    }

    /// Wound-Wait 裁决：TxId 越小越老
    pub fn is_older(&self, tx1: TxId, tx2: TxId) -> bool {
        tx1 < tx2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_begin_returns_increasing_ids() {
        let tm = TransactionManager::new();
        let id1 = tm.begin();
        let id2 = tm.begin();
        assert!(id1 < id2, "TxId 必须单调递增");
    }

    #[test]
    fn test_mark_committed() {
        let tm = TransactionManager::new();
        let id = tm.begin();
        assert!(tm.is_active(id));
        tm.mark_committed(id);
        assert!(!tm.is_active(id));
    }

    #[test]
    fn test_mark_aborted() {
        let tm = TransactionManager::new();
        let id = tm.begin();
        assert!(tm.is_active(id));
        tm.mark_aborted(id);
        assert!(!tm.is_active(id));
    }

    #[test]
    fn test_is_older() {
        let tm = TransactionManager::new();
        let id1 = tm.begin();
        let id2 = tm.begin();
        assert!(tm.is_older(id1, id2));
        assert!(!tm.is_older(id2, id1));
    }

    #[test]
    fn test_concurrent_begin_unique_ids() {
        let tm = Arc::new(TransactionManager::new());
        let mut handles = vec![];
        for _ in 0..10 {
            let tm_clone = Arc::clone(&tm);
            handles.push(thread::spawn(move || {
                let mut ids = vec![];
                for _ in 0..100 {
                    ids.push(tm_clone.begin());
                }
                ids
            }));
        }
        let mut all_ids = vec![];
        for h in handles {
            all_ids.extend(h.join().unwrap());
        }
        all_ids.sort();
        all_ids.dedup();
        assert_eq!(all_ids.len(), 1000, "1000 个 TxId 必须全部唯一");
    }
}
