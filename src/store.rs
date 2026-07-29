// Veritas Kernel - Store 模块
// Phase 6: 状态存储独立管理

use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{StateId, StateEntry, Version};

/// 状态存储：管理所有状态的最新版本
pub struct StateStore {
    map: Mutex<HashMap<StateId, StateEntry>>,
}

impl StateStore {
    /// 创建新的空存储
    pub fn new() -> Self {
        StateStore {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// 从已有的 HashMap 构建
    pub fn from_map(map: HashMap<StateId, StateEntry>) -> Self {
        StateStore {
            map: Mutex::new(map),
        }
    }

    /// 读取状态（返回克隆值）
    pub fn read(&self, state_id: StateId) -> Option<StateEntry> {
        let map = self.map.lock().unwrap();
        map.get(&state_id).cloned()
    }

    /// 插入或更新状态
    pub fn insert(&self, state_id: StateId, entry: StateEntry) {
        let mut map = self.map.lock().unwrap();
        map.insert(state_id, entry);
    }

    /// 获取状态的版本号
    pub fn version(&self, state_id: StateId) -> Option<Version> {
        let map = self.map.lock().unwrap();
        map.get(&state_id).map(|e| e.version)
    }

    /// 检查状态是否存在
    pub fn exists(&self, state_id: StateId) -> bool {
        let map = self.map.lock().unwrap();
        map.contains_key(&state_id)
    }

    /// 获取所有状态（用于恢复）
    pub fn into_inner(self) -> HashMap<StateId, StateEntry> {
        self.map.into_inner().unwrap()
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}
