use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use crate::types::{ObjectId, TxId};
use crate::tx_manager::TransactionManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Shared,
    Exclusive,
}

struct LockEntry {
    mode: LockMode,
    holders: HashSet<TxId>,
}

pub struct LockManager {
    locks: Mutex<HashMap<ObjectId, LockEntry>>,
    tx_held: Mutex<HashMap<TxId, HashSet<ObjectId>>>,
    tx_mgr: Arc<TransactionManager>,
}

impl LockManager {
    pub fn new(tx_mgr: Arc<TransactionManager>) -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            tx_held: Mutex::new(HashMap::new()),
            tx_mgr,
        }
    }

    /// Wound-Wait 锁申请
    pub fn acquire(
        &self,
        tx_id: TxId,
        obj_id: ObjectId,
        mode: LockMode,
    ) -> Result<(), String> {
        let mut locks = self.locks.lock().unwrap();

        // 如果当前事务已被击毙，直接拒绝
        if !self.tx_mgr.is_active(tx_id) {
            return Err(format!("TxId={} has been aborted", tx_id));
        }

        let entry = locks.entry(obj_id).or_insert(LockEntry {
            mode,
            holders: HashSet::new(),
        });

        // 无冲突或同事务重入
        if entry.holders.is_empty() || entry.holders.contains(&tx_id) {
            entry.mode = mode;
            entry.holders.insert(tx_id);
            let mut tx_held = self.tx_held.lock().unwrap();
            tx_held.entry(tx_id).or_default().insert(obj_id);
            return Ok(());
        }

        // Shared + Shared 兼容，直接授予
        if entry.mode == LockMode::Shared && mode == LockMode::Shared {
            entry.holders.insert(tx_id);
            let mut tx_held = self.tx_held.lock().unwrap();
            tx_held.entry(tx_id).or_default().insert(obj_id);
            return Ok(());
        }

        // --- Wound-Wait 裁决 ---
        // 收集冲突持有者（避免迭代中修改）
        let conflict_holders: Vec<TxId> = entry.holders.iter().copied().collect();

        for holder_id in conflict_holders {
            if holder_id == tx_id {
                continue;
            }

            if self.tx_mgr.is_older(tx_id, holder_id) {
                // Wound：当前事务更老，击毙持有者（新事务）
                self.tx_mgr.mark_aborted(holder_id);
                entry.holders.remove(&holder_id);
                // 释放被击毙事务的全部锁
                drop(locks);
                self.release_all(holder_id);
                locks = self.locks.lock().unwrap();
                // 重新获取 entry 引用（release_all 可能已清理）
                let entry = locks.entry(obj_id).or_insert(LockEntry {
                    mode,
                    holders: HashSet::new(),
                });
                entry.holders.insert(tx_id);
                entry.mode = mode;
                let mut tx_held = self.tx_held.lock().unwrap();
                tx_held.entry(tx_id).or_default().insert(obj_id);
                return Ok(());
            } else {
                // Die：当前事务更新，避让
                return Err(format!(
                    "Lock conflict: TxId={} cannot acquire Object={}, held by older TxId={}",
                    tx_id, obj_id, holder_id
                ));
            }
        }

        // 清理完冲突持有者后授予锁
        entry.mode = mode;
        entry.holders.insert(tx_id);
        let mut tx_held = self.tx_held.lock().unwrap();
        tx_held.entry(tx_id).or_default().insert(obj_id);
        Ok(())
    }

    /// 释放事务持有的全部锁
    pub fn release_all(&self, tx_id: TxId) {
        let mut tx_held = self.tx_held.lock().unwrap();
        if let Some(objs) = tx_held.remove(&tx_id) {
            let mut locks = self.locks.lock().unwrap();
            for obj_id in objs {
                if let Some(entry) = locks.get_mut(&obj_id) {
                    entry.holders.remove(&tx_id);
                    if entry.holders.is_empty() {
                        locks.remove(&obj_id);
                    }
                }
            }
        }
    }

    pub fn is_locked(&self, obj_id: ObjectId) -> bool {
        let locks = self.locks.lock().unwrap();
        locks.contains_key(&obj_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup() -> (Arc<TransactionManager>, LockManager, TxId, TxId) {
        let tm = Arc::new(TransactionManager::new());
        let lm = LockManager::new(Arc::clone(&tm));
        let tx1 = tm.begin();
        let tx2 = tm.begin();
        (tm, lm, tx1, tx2)
    }

    #[test]
    fn test_shared_compatible() {
        let (_tm, lm, tx1, tx2) = setup();
        assert!(lm.acquire(tx1, 100, LockMode::Shared).is_ok());
        assert!(lm.acquire(tx2, 100, LockMode::Shared).is_ok());
    }

    #[test]
    fn test_exclusive_conflict() {
        let (_tm, lm, tx1, tx2) = setup();
        lm.acquire(tx1, 100, LockMode::Exclusive).unwrap();
        assert!(lm.acquire(tx2, 100, LockMode::Shared).is_err());
    }

    #[test]
    fn test_same_tx_reacquire() {
        let (_tm, lm, tx1, _tx2) = setup();
        lm.acquire(tx1, 100, LockMode::Shared).unwrap();
        assert!(lm.acquire(tx1, 100, LockMode::Exclusive).is_ok());
    }

    #[test]
    fn test_release_all_frees() {
        let (_tm, lm, tx1, _tx2) = setup();
        lm.acquire(tx1, 100, LockMode::Exclusive).unwrap();
        lm.release_all(tx1);
        assert!(!lm.is_locked(100));
    }

    #[test]
    fn test_wound_older_kills_newer() {
        let (tm, lm, tx1, tx2) = setup();
        let young_id = tx1; // 老
        let old_id = tx2;   // 新 — 注意命名反了但逻辑对

        // 新事务先持锁
        lm.acquire(old_id, 100, LockMode::Exclusive).unwrap();

        // 老事务抢锁 → Wound：击毙新事务
        assert!(lm.acquire(young_id, 100, LockMode::Exclusive).is_ok());

        // 新事务已被标记 Aborted
        assert!(!tm.is_active(old_id));
    }

    #[test]
    fn test_die_newer_yields_to_older() {
        let (tm, lm, tx1, tx2) = setup();
        let old_id = tx1;   // 老
        let young_id = tx2; // 新

        // 老事务持锁
        lm.acquire(old_id, 100, LockMode::Exclusive).unwrap();

        // 新事务抢锁 → Die：被拒绝
        assert!(lm.acquire(young_id, 100, LockMode::Exclusive).is_err());

        // 老事务仍然存活
        assert!(tm.is_active(old_id));
    }

    #[test]
    fn test_wound_cascades_lock_release() {
        let (tm, lm, tx1, tx2) = setup();
        let young_id = tx1; // 老
        let old_id = tx2;   // 新

        // 新事务锁住两个资源
        lm.acquire(old_id, 100, LockMode::Exclusive).unwrap();
        lm.acquire(old_id, 200, LockMode::Exclusive).unwrap();

        // 老事务抢其中一个 → Wound 击毙新事务，释放其全部锁
        assert!(lm.acquire(young_id, 100, LockMode::Exclusive).is_ok());

        // 新事务的所有锁都已释放
        assert!(!lm.is_locked(200));
    }
}
