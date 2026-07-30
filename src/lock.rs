use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use crate::types::{ObjectId, TxId};

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
    /// ObjectId -> 当前锁状态
    locks: Mutex<HashMap<ObjectId, LockEntry>>,
    /// TxId -> 该事务持有的所有 ObjectId
    tx_held: Mutex<HashMap<TxId, HashSet<ObjectId>>>,
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            tx_held: Mutex::new(HashMap::new()),
        }
    }

    /// 申请锁。Shared 与 Shared 兼容，Exclusive 与任何模式互斥。
    /// 同一事务重复申请同一资源不冲突。
    pub fn acquire(
        &self,
        tx_id: TxId,
        obj_id: ObjectId,
        mode: LockMode,
    ) -> Result<(), String> {
        let mut locks = self.locks.lock().unwrap();
        let entry = locks.entry(obj_id).or_insert(LockEntry {
            mode,
            holders: HashSet::new(),
        });

        // 同一事务已持有，直接成功
        if entry.holders.contains(&tx_id) {
            entry.mode = mode; // 可能升级
            return Ok(());
        }

        // 冲突检测
        if !entry.holders.is_empty() {
            match (entry.mode, mode) {
                (LockMode::Shared, LockMode::Shared) => {}
                _ => {
                    return Err(format!(
                        "Lock conflict: TxId={} on Object={}, held by {:?}",
                        tx_id, obj_id, entry.holders
                    ));
                }
            }
        }

        // 授予锁
        entry.mode = mode;
        entry.holders.insert(tx_id);

        let mut tx_held = self.tx_held.lock().unwrap();
        tx_held.entry(tx_id).or_default().insert(obj_id);

        Ok(())
    }

    /// 释放指定事务持有的全部锁
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

    /// 查询某个 Object 当前是否被锁定
    pub fn is_locked(&self, obj_id: ObjectId) -> bool {
        let locks = self.locks.lock().unwrap();
        locks.contains_key(&obj_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acquire_shared_compatible() {
        let lm = LockManager::new();
        assert!(lm.acquire(1, 100, LockMode::Shared).is_ok());
        assert!(lm.acquire(2, 100, LockMode::Shared).is_ok());
    }

    #[test]
    fn test_acquire_exclusive_conflict() {
        let lm = LockManager::new();
        assert!(lm.acquire(1, 100, LockMode::Exclusive).is_ok());
        assert!(lm.acquire(2, 100, LockMode::Shared).is_err());
    }

    #[test]
    fn test_same_tx_reacquire_no_conflict() {
        let lm = LockManager::new();
        assert!(lm.acquire(1, 100, LockMode::Shared).is_ok());
        assert!(lm.acquire(1, 100, LockMode::Exclusive).is_ok());
    }

    #[test]
    fn test_release_all_frees_locks() {
        let lm = LockManager::new();
        lm.acquire(1, 100, LockMode::Exclusive).unwrap();
        lm.release_all(1);
        assert!(!lm.is_locked(100));
        assert!(lm.acquire(2, 100, LockMode::Exclusive).is_ok());
    }

    #[test]
    fn test_release_all_spares_other_tx() {
        let lm = LockManager::new();
        lm.acquire(1, 100, LockMode::Shared).unwrap();
        lm.acquire(2, 100, LockMode::Shared).unwrap();
        lm.release_all(1);
        assert!(lm.is_locked(100)); // TxId=2 仍持有
        lm.release_all(2);
        assert!(!lm.is_locked(100));
    }
}
