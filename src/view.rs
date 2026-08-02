use crate::types::ObjectId;
use std::collections::HashMap;

/// 当前事务视角下的 Object 世界视图。
/// Guard 只依赖此 trait，不绑定具体存储实现。
pub trait ObjectView {
    fn is_alive(&self, id: ObjectId) -> bool;
    fn is_dead(&self, id: ObjectId) -> bool;
    fn exists(&self, id: ObjectId) -> bool;
}

/// 融合 registry + pending_births + pending_deaths 的事务视图。
/// pending_births 中的对象视为 Alive，pending_deaths 中的对象视为 Dead。
pub struct TransactionObjectView<'a> {
    registry: &'a HashMap<ObjectId, crate::types::ObjectRecord>,
    pending_births: &'a [ObjectId],
    pending_deaths: &'a [ObjectId],
}

impl<'a> TransactionObjectView<'a> {
    pub fn new(
        registry: &'a HashMap<ObjectId, crate::types::ObjectRecord>,
        pending_births: &'a [ObjectId],
        pending_deaths: &'a [ObjectId],
    ) -> Self {
        Self { registry, pending_births, pending_deaths }
    }
}

impl<'a> ObjectView for TransactionObjectView<'a> {
    fn is_alive(&self, id: ObjectId) -> bool {
        if self.pending_deaths.contains(&id) {
            return false;
        }
        if self.pending_births.contains(&id) {
            return true;
        }
        self.registry.get(&id).map(|r| r.is_alive()).unwrap_or(false)
    }

    fn is_dead(&self, id: ObjectId) -> bool {
        if self.pending_deaths.contains(&id) {
            return true;
        }
        if self.pending_births.contains(&id) {
            return false;
        }
        self.registry.get(&id).map(|r| r.is_dead()).unwrap_or(false)
    }

    fn exists(&self, id: ObjectId) -> bool {
        self.is_alive(id) || self.is_dead(id)
    }
}
