// Veritas Kernel - INVARIANT_SCOPE (ScopeRegistry backed)
//
// 关键变化：bind/unbind 不再直接改 registry，而是写入
// ctx.scope_write_set，commit 时才真正应用——这样 rollback_to
// 才能正确撤销 scope 结构变更。

use crate::engine::VeritasEngine;
use crate::types::*;

pub fn scope_state_id(scope_name: &str) -> ScopeId {
    deterministic_hash(&format!("__scope__:{}", scope_name))
}

pub trait ScopeExt {
    fn declare_scope(&self, scope_name: &str);

    fn enumerate_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<Vec<StateId>, VeritasError>;

    fn touch_scope_read(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<(), VeritasError> {
        self.enumerate_scope(ctx, scope_name).map(|_| ())
    }

    fn bind_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
        member: StateId,
    ) -> Result<(), VeritasError>;

    fn unbind_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
        member: StateId,
    ) -> Result<(), VeritasError>;
}

impl ScopeExt for VeritasEngine {
    fn declare_scope(&self, scope_name: &str) {
        let id = scope_state_id(scope_name);
        self.scope_registry().declare(id, 0);
    }

    fn enumerate_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
    ) -> Result<Vec<StateId>, VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let id = scope_state_id(scope_name);
        let (members, struct_version) = self
            .scope_registry()
            .snapshot(id)
            .ok_or_else(|| {
                VeritasError::EngineError(format!("scope '{}' not found", scope_name))
            })?;

        ctx.read_set.scopes.insert(id, struct_version);
        Ok(members)
    }

    fn bind_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
        member: StateId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let id = scope_state_id(scope_name);
        if !self.scope_registry().exists(id) {
            return Err(VeritasError::EngineError(format!(
                "scope '{}' not found",
                scope_name
            )));
        }
        ctx.scope_write_set.push(ScopeChange {
            scope_id: id,
            state_id: member,
            change_type: ScopeChangeType::Bind,
        });
        Ok(())
    }

    fn unbind_scope(
        &self,
        ctx: &mut TransactionContext,
        scope_name: &str,
        member: StateId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let id = scope_state_id(scope_name);
        if !self.scope_registry().exists(id) {
            return Err(VeritasError::EngineError(format!(
                "scope '{}' not found",
                scope_name
            )));
        }
        ctx.scope_write_set.push(ScopeChange {
            scope_id: id,
            state_id: member,
            change_type: ScopeChangeType::Unbind,
        });
        Ok(())
    }
}
