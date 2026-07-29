// Veritas Kernel - Extension 系统

use crate::types::{StateId, AbortReason, TransactionContext};

pub trait Extension: Send + Sync {
    fn before_begin(&self, _ctx: &mut TransactionContext) -> Result<(), AbortReason> {
        Ok(())
    }

    fn before_read(
        &self,
        _ctx: &mut TransactionContext,
        _state: StateId,
    ) -> Result<(), AbortReason> {
        Ok(())
    }

    fn before_write(
        &self,
        _ctx: &mut TransactionContext,
        _state: StateId,
    ) -> Result<(), AbortReason> {
        Ok(())
    }

    fn before_commit(&self, _ctx: &TransactionContext) -> Result<(), AbortReason> {
        Ok(())
    }

    fn after_commit(&self, _ctx: &TransactionContext) {
    }

    fn after_abort(&self, _ctx: &TransactionContext) {
    }
}
