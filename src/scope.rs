// Veritas Kernel - Phase 4: INVARIANT_SCOPE
//
// 设计原则：Scope不需要独立的检测机制。
// 每个Scope是一个"幽灵状态"，其value无业务含义，version代表结构变更次数。
// 聚合查询=读幽灵状态（纳入read_set），成员变更=写幽灵状态（推高version）。
// 现有的 commit() 冲突检测原样复用，无需修改 engine.rs 核心逻辑。

use crate::engine::VeritasEngine;
use crate::types::*;
use crate::transaction::TransactionContext;

pub fn scope_state_id(scope_name: &str) -> StateId {
    deterministic_hash(&format!("__scope__:{}", scope_name))
}

pub trait ScopeExt {
    fn declare_scope(&self, scope_name: &str);

    fn touch_scope_read(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<(), VeritasError>;

    fn touch_scope_write(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<(), VeritasError>;
}

impl ScopeExt for VeritasEngine {
    fn declare_scope(&self, scope_name: &str) {
        let id = scope_state_id(scope_name);
        if self.peek_state(id).is_none() {
            self.init_state(id, 0u64.to_le_bytes().to_vec());
        }
    }

    fn touch_scope_read(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<(), VeritasError> {
        let id = scope_state_id(scope_name);
        self.read(ctx, id)?;
        Ok(())
    }

    fn touch_scope_write(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<(), VeritasError> {
        let id = scope_state_id(scope_name);
        let current = self.read(ctx, id)?;
        let v = u64::from_le_bytes(current[..8].try_into().unwrap());
        self.write(ctx, id, (v + 1).to_le_bytes().to_vec())?;
        Ok(())
    }
}
