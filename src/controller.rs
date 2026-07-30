use std::sync::Arc;
use crate::types::{TxId, VeritasError, AbortReason, TransactionContext};
use crate::tx_manager::TransactionManager;
use crate::lock::LockManager;

/// 事务执行控制器：负责事务生命周期管理，
/// 将控制逻辑从 Engine 中剥离。
pub struct TransactionController {
    pub tx_mgr: Arc<TransactionManager>,
    pub lock_mgr: Arc<LockManager>,
}

impl TransactionController {
    pub fn new(
        tx_mgr: Arc<TransactionManager>,
        lock_mgr: Arc<LockManager>,
    ) -> Self {
        Self { tx_mgr, lock_mgr }
    }

    /// 开启事务：分配 TxId + 创建上下文
    pub fn begin(&self, snapshot_version: u64) -> TransactionContext {
        let tx_id = self.tx_mgr.begin();
        TransactionContext::new(tx_id, snapshot_version)
    }

    /// 提交前校验 PCB 状态
    pub fn pre_commit_check(&self, ctx: &TransactionContext) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        if !self.tx_mgr.is_active(ctx.tx_id()) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        Ok(())
    }

    /// 提交成功后的清理
    pub fn post_commit(&self, tx_id: TxId) {
        self.tx_mgr.mark_committed(tx_id);
        self.lock_mgr.release_all(tx_id);
        self.tx_mgr.remove(tx_id);
    }

    /// 终止事务
    pub fn abort(&self, ctx: &mut TransactionContext, reason: AbortReason) {
        ctx.set_aborted();
        self.tx_mgr.mark_aborted(ctx.tx_id());
        self.lock_mgr.release_all(ctx.tx_id());
        self.tx_mgr.remove(ctx.tx_id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup() -> TransactionController {
        let tx_mgr = Arc::new(TransactionManager::new());
        let lock_mgr = Arc::new(LockManager::new(Arc::clone(&tx_mgr)));
        TransactionController::new(tx_mgr, lock_mgr)
    }

    #[test]
    fn test_controller_begin_creates_active_tx() {
        let ctrl = setup();
        let ctx = ctrl.begin(1);
        assert!(ctrl.tx_mgr.is_active(ctx.tx_id()),
            "begin 后事务应为 Active");
    }

    #[test]
    fn test_controller_abort_marks_inactive() {
        let ctrl = setup();
        let mut ctx = ctrl.begin(1);
        let id = ctx.tx_id();
        ctrl.abort(&mut ctx, AbortReason::WriteConflict);
        assert!(!ctrl.tx_mgr.is_active(id),
            "abort 后事务不应为 Active");
    }

    #[test]
    fn test_controller_pre_commit_rejects_aborted() {
        let ctrl = setup();
        let mut ctx = ctrl.begin(1);
        ctrl.abort(&mut ctx, AbortReason::WriteConflict);
        assert!(ctrl.pre_commit_check(&ctx).is_err(),
            "已 abort 的事务应被 pre_commit_check 拒绝");
    }

    #[test]
    fn test_controller_post_commit_cleans_up() {
        let ctrl = setup();
        let ctx = ctrl.begin(1);
        let id = ctx.tx_id();
        ctrl.post_commit(id);
        assert!(!ctrl.tx_mgr.is_active(id),
            "post_commit 后事务应结束");
    }
}
