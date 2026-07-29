// Veritas Kernel V0.1 - 事务引擎核心
// Phase 1.1: 修正读自己的写、fail-fast快照隔离、盲写保护、原子版本号
//
// 与V0.4设计文档的差异（已知局限）：
// - 采用fail-fast OCC近似快照隔离，不维护历史版本链
// - 事务读取到超前版本时立即ABORT，而非返回BEGIN时的快照值
// - 真MVCC版本链留待后续性能阶段实现
// - 冲突ABORT后由上层负责重试（默认指数退避+随机抖动）

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::types::*;

/// Veritas 事务引擎
pub struct VeritasEngine {
    /// 全局版本号（原子递增）
    global_version: AtomicU64,
    /// 状态存储（仅保存最新版本，无历史版本链）
    state_store: Mutex<HashMap<StateId, StateEntry>>,
    /// 事务ID计数器
    tx_id_counter: AtomicU64,
    /// 提交锁（全局提交临界区）
    commit_lock: Mutex<()>,
}

impl VeritasEngine {
    /// 创建新的引擎实例
    pub fn new() -> Self {
        VeritasEngine {
            global_version: AtomicU64::new(0),
            state_store: Mutex::new(HashMap::new()),
            tx_id_counter: AtomicU64::new(1),
            commit_lock: Mutex::new(()),
        }
    }

    /// 初始化状态：注册一个状态并设置初始值
    pub fn init_state(&self, state_id: StateId, initial_value: Vec<u8>) {
        let mut store = self.state_store.lock().unwrap();
        store.insert(
            state_id,
            StateEntry {
                value: initial_value,
                version: 0,
            },
        );
    }

    /// 开始新事务
    pub fn begin(&self) -> TransactionContext {
        let tx_id = self.tx_id_counter.fetch_add(1, Ordering::SeqCst);
        let snapshot_version = self.global_version.load(Ordering::Acquire);

        TransactionContext {
            tx_id,
            snapshot_version,
            read_set: ReadSet::default(),
            write_set: WriteSet::default(),
            aborted: false,
        }
    }

    /// 读取状态
    ///
    /// 读取顺序：
    /// 1. 先查WriteSet（读自己的写，不记录到ReadSet）
    /// 2. 再查全局Store
    /// 3. 若全局版本 > 快照版本，fail-fast ABORT（无历史版本链）
    /// 4. 自动记录到ReadSet
    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        if ctx.aborted {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 【修正1：读自己的写】
        if let Some(written_value) = ctx.write_set.state_changes.get(&state_id) {
            return Ok(written_value.clone());
        }

        // 从全局Store读取
        let store = self.state_store.lock().unwrap();
        let entry = store
            .get(&state_id)
            .ok_or(VeritasError::EngineError(format!(
                "State {:?} not found",
                state_id
            )))?;

        // 【修正2：fail-fast快照隔离】
        // 无历史版本链，读到超前版本直接ABORT
        if entry.version > ctx.snapshot_version {
            return Err(VeritasError::Abort(AbortReason::ReadFutureVersion));
        }

        // 自动记录到ReadSet
        ctx.read_set.states.insert(state_id, entry.version);

        Ok(entry.value.clone())
    }

    /// 写入状态（暂存到WriteSet）
    ///
    /// 【盲写保护】若该state尚未在ReadSet中，先将其当前版本补入ReadSet
    /// 这确保"写-写冲突"也能被commit时的detect_conflict捕获
    pub fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) -> Result<(), VeritasError> {
        if ctx.aborted {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 【盲写保护】写前隐式记录当前版本到ReadSet
        if !ctx.read_set.states.contains_key(&state_id) {
            let store = self.state_store.lock().unwrap();
            if let Some(entry) = store.get(&state_id) {
                ctx.read_set.states.insert(state_id, entry.version);
            }
        }

        ctx.write_set.state_changes.insert(state_id, value);
        Ok(())
    }

    /// 提交事务
    pub fn commit(&self, ctx: &mut TransactionContext) -> Result<(), VeritasError> {
        if ctx.aborted {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 【进入全局提交临界区】
        let _lock = self.commit_lock.lock().unwrap();

        // 步骤1：冲突检测
        self.detect_conflict(ctx)?;

        // 步骤2：【修正3：原子递增】fetch_add返回加之前的值
        let commit_version = self.global_version.fetch_add(1, Ordering::SeqCst) + 1;

        // 步骤3：状态固化
        {
            let mut store = self.state_store.lock().unwrap();
            for (state_id, new_value) in &ctx.write_set.state_changes {
                if let Some(entry) = store.get_mut(state_id) {
                    entry.value = new_value.clone();
                    entry.version = commit_version;
                }
            }
        }
        // fetch_add已原子递增，不需要再store
        // 【退出临界区】
        drop(_lock);

        Ok(())
    }

    /// 中止事务
    pub fn abort(&self, ctx: &mut TransactionContext, _reason: AbortReason) {
        ctx.aborted = true;
    }

    /// 冲突检测（读-写反依赖 + 写-写冲突）
    fn detect_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        let store = self.state_store.lock().unwrap();
        for (state_id, read_version) in &ctx.read_set.states {
            if let Some(entry) = store.get(state_id) {
                if entry.version > *read_version {
                    return Err(AbortReason::WriteConflict);
                }
            }
        }
        Ok(())
    }

    /// 获取当前全局版本号（调试用）
    pub fn get_global_version(&self) -> Version {
        self.global_version.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助函数：创建带初始值的引擎
    fn setup_engine() -> (VeritasEngine, StateId) {
        let engine = VeritasEngine::new();
        let state_a = deterministic_hash("Account::A::Balance");
        engine.init_state(state_a, 100u64.to_le_bytes().to_vec());
        (engine, state_a)
    }

    /// 辅助函数：将字节数组解析为u64
    fn bytes_to_u64(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
    }

    // ==================== Phase 1.1 新增测试 ====================

    /// 测试：读自己的写（Read Your Writes）
    #[test]
    fn test_read_your_writes() {
        let (engine, state_a) = setup_engine();

        let mut ctx = engine.begin();

        // 先写入新值
        engine
            .write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();

        // 再读取——应该读到刚写的50，而非全局的100
        let balance = engine.read(&mut ctx, state_a).unwrap();
        assert_eq!(bytes_to_u64(&balance), 50);

        // 提交后验证全局状态
        engine.commit(&mut ctx).unwrap();

        let mut ctx2 = engine.begin();
        let final_val = engine.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&final_val), 50);
    }

    /// 测试：盲写冲突检测（两个事务都不读直接写）
    #[test]
    fn test_blind_write_conflict() {
        let engine = VeritasEngine::new();
        let state_a = deterministic_hash("Account::A::Balance");
        engine.init_state(state_a, 100u64.to_le_bytes().to_vec());

        // 事务1：盲写A=50
        let mut ctx1 = engine.begin();
        engine
            .write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();

        // 事务2：盲写A=30，先提交
        let mut ctx2 = engine.begin();
        engine
            .write(&mut ctx2, state_a, 30u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx2).unwrap();

        // 事务1提交——应该检测到冲突
        let result = engine.commit(&mut ctx1);
        assert!(result.is_err());
        match result {
            Err(VeritasError::Abort(AbortReason::WriteConflict)) => {}
            other => panic!("Expected WriteConflict, got {:?}", other),
        }

        // 最终值应该是事务2的30，而非事务1的50覆盖
        let mut ctx3 = engine.begin();
        let final_val = engine.read(&mut ctx3, state_a).unwrap();
        assert_eq!(bytes_to_u64(&final_val), 30);
    }

    /// 测试：fail-fast快照隔离——读到超前版本立即ABORT
    #[test]
    fn test_read_future_version_aborts() {
        let (engine, state_a) = setup_engine();

        // 事务1：BEGIN，快照版本=0
        let mut ctx1 = engine.begin();
        assert_eq!(ctx1.snapshot_version, 0);

        // 事务2：修改A并提交，版本变成1
        let mut ctx2 = engine.begin();
        engine
            .write(&mut ctx2, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx2).unwrap();

        // 事务1：尝试读A——全局版本1 > 快照版本0，应该fail-fast ABORT
        let result = engine.read(&mut ctx1, state_a);
        assert!(result.is_err());
        match result {
            Err(VeritasError::Abort(AbortReason::ReadFutureVersion)) => {}
            other => panic!("Expected ReadFutureVersion, got {:?}", other),
        }
    }

    // ==================== Phase 1.0 原有测试 ====================

    /// 基本事务流程
    #[test]
    fn test_basic_transaction() {
        let (engine, state_a) = setup_engine();

        let mut ctx = engine.begin();
        let balance = engine.read(&mut ctx, state_a).unwrap();
        assert_eq!(bytes_to_u64(&balance), 100);

        engine
            .write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx).unwrap();

        let mut ctx2 = engine.begin();
        let final_val = engine.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&final_val), 50);
    }

    /// 写冲突检测
    #[test]
    fn test_write_conflict() {
        let (engine, state_a) = setup_engine();

        let mut ctx1 = engine.begin();
        let _ = engine.read(&mut ctx1, state_a).unwrap();

        let mut ctx2 = engine.begin();
        let _ = engine.read(&mut ctx2, state_a).unwrap();
        engine
            .write(&mut ctx2, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx2).unwrap();

        engine
            .write(&mut ctx1, state_a, 30u64.to_le_bytes().to_vec())
            .unwrap();
        let result = engine.commit(&mut ctx1);
        assert!(result.is_err());
        match result {
            Err(VeritasError::Abort(AbortReason::WriteConflict)) => {}
            other => panic!("Expected WriteConflict, got {:?}", other),
        }
    }

    /// 隔离性：未提交修改对其他事务不可见
    #[test]
    fn test_isolation() {
        let (engine, state_a) = setup_engine();

        let mut ctx1 = engine.begin();
        engine
            .write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();

        let mut ctx2 = engine.begin();
        let balance = engine.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&balance), 100); // 仍是100
    }
}
