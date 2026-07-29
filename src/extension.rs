// Veritas Kernel - Extension 系统
// Phase 6: 插件化架构

use crate::types::{StateId, AbortReason};
use crate::transaction::TransactionContext;

/// Extension trait：所有扩展的基类
/// 每个扩展可以在事务生命周期的不同阶段插入逻辑
pub trait Extension: Send + Sync {
    /// 事务开始前调用
    fn before_begin(&self, _ctx: &mut TransactionContext) -> Result<(), AbortReason> {
        Ok(())
    }

    /// 读取状态前调用
    fn before_read(
        &self,
        _ctx: &mut TransactionContext,
        _state: StateId,
    ) -> Result<(), AbortReason> {
        Ok(())
    }

    /// 写入状态前调用
    fn before_write(
        &self,
        _ctx: &mut TransactionContext,
        _state: StateId,
    ) -> Result<(), AbortReason> {
        Ok(())
    }

    /// 提交前调用（在冲突检测之前）
    fn before_commit(&self, _ctx: &TransactionContext) -> Result<(), AbortReason> {
        Ok(())
    }

    /// 提交后调用（在 WAL 写入和状态固化之后）
    fn after_commit(&self, _ctx: &TransactionContext) {
    }

    /// 中止后调用
    fn after_abort(&self, _ctx: &TransactionContext) {
    }
}
