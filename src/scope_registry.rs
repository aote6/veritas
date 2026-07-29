// Veritas Kernel - ScopeRegistry
// Scope 是一等公民：独立的成员集合 + 独立的结构版本号，
// 用于和 state 的 value version 区分开来。

use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::{ModuleId, ScopeEntry, ScopeId, StateId, Version};

pub struct ScopeRegistry {
    scopes: RwLock<HashMap<ScopeId, ScopeEntry>>,
}

impl ScopeRegistry {
    pub fn new() -> Self {
        ScopeRegistry {
            scopes: RwLock::new(HashMap::new()),
        }
    }

    /// 恢复时用重放出的 map 直接构建
    pub fn from_map(map: HashMap<ScopeId, ScopeEntry>) -> Self {
        ScopeRegistry {
            scopes: RwLock::new(map),
        }
    }

    /// 幂等声明：已存在则不覆盖
    pub fn declare(&self, scope_id: ScopeId, owner: ModuleId) {
        let mut map = self.scopes.write().unwrap();
        map.entry(scope_id).or_insert_with(|| {
            let mut e = ScopeEntry::new();
            e.owner = owner;
            e
        });
    }

    pub fn exists(&self, scope_id: ScopeId) -> bool {
        self.scopes.read().unwrap().contains_key(&scope_id)
    }

    /// enumerate_scope 用：成员快照 + 当前结构版本号
    pub fn snapshot(&self, scope_id: ScopeId) -> Option<(Vec<StateId>, Version)> {
        self.scopes
            .read()
            .unwrap()
            .get(&scope_id)
            .map(|e| (e.members.clone(), e.struct_version))
    }

    pub fn struct_version(&self, scope_id: ScopeId) -> Option<Version> {
        self.scopes
            .read()
            .unwrap()
            .get(&scope_id)
            .map(|e| e.struct_version)
    }

    /// 只在 commit 阶段调用，真正落地结构变更
    pub fn apply_bind(&self, scope_id: ScopeId, state_id: StateId) {
        if let Some(entry) = self.scopes.write().unwrap().get_mut(&scope_id) {
            entry.bind(state_id);
        }
    }

    pub fn apply_unbind(&self, scope_id: ScopeId, state_id: StateId) {
        if let Some(entry) = self.scopes.write().unwrap().get_mut(&scope_id) {
            entry.unbind(state_id);
        }
    }
}

impl Default for ScopeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
