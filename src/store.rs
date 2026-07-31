// Veritas Kernel - Store 模块
// Phase 6: 状态存储独立管理
// 重构：键从裸StateId改为Address(ObjectId, StateId)二元寻址
// 依据 memory.md 第4节："地址 = (ObjectId, StateId)，没有全局地址"

use std::collections::HashMap;
use std::sync::Mutex;

use crate::types::{Address, StateEntry, Version};

/// 状态存储：管理所有状态的最新版本，按(ObjectId, StateId)寻址
pub struct StateStore {
    map: Mutex<HashMap<Address, StateEntry>>,
}

impl StateStore {
    /// 创建新的空存储
    pub fn new() -> Self {
        StateStore {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// 从已有的 HashMap 构建
    pub fn from_map(map: HashMap<Address, StateEntry>) -> Self {
        StateStore {
            map: Mutex::new(map),
        }
    }

    /// 读取状态（返回克隆值）
    pub fn read(&self, addr: Address) -> Option<StateEntry> {
        let map = self.map.lock().unwrap();
        map.get(&addr).cloned()
    }

    /// 插入或更新状态
    pub fn insert(&self, addr: Address, entry: StateEntry) {
        let mut map = self.map.lock().unwrap();
        map.insert(addr, entry);
    }

    /// 获取状态的版本号
    pub fn version(&self, addr: Address) -> Option<Version> {
        let map = self.map.lock().unwrap();
        map.get(&addr).map(|e| e.version)
    }

    /// 检查状态是否存在
    pub fn exists(&self, addr: Address) -> bool {
        let map = self.map.lock().unwrap();
        map.contains_key(&addr)
    }

    /// 获取所有状态（用于恢复）
    pub fn into_inner(self) -> HashMap<Address, StateEntry> {
        self.map.into_inner().unwrap()
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}
