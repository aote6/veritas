// Veritas Kernel V0.3 - 事务引擎核心
// P1: WAL 格式扩展 + Effect 崩溃恢复重试 + tx_id_counter 恢复续接

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::scope_registry::ScopeRegistry;
use crate::types::*;
use crate::wal::{RecoveryManager, WalEffect, WalEntry, WalScopeChange, WalWriter};
use crate::store::StateStore;

fn bytes_to_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}

const WAL_PATH: &str = "wal.log";

pub struct VeritasEngine {
    global_version: AtomicU64,
    state_store: StateStore,
    scope_registry: ScopeRegistry,
    tx_id_counter: AtomicU64,
    commit_lock: Mutex<()>,
    wal: WalWriter,
    object_registry: Mutex<HashSet<ObjectId>>,
}

impl VeritasEngine {
    pub fn new() -> Self {
        Self::with_wal_path(WAL_PATH.to_string())
    }

    pub fn with_wal_path(wal_path: String) -> Self {
        let (records, recovered_version) = RecoveryManager::recover(&wal_path)
            .unwrap_or_else(|e| {
                eprintln!("[WARN] WAL recovery failed: {}, starting fresh", e);
                (Vec::new(), 0)
            });

        let (state_map, scope_map, pending_effects, max_tx_id) =
            RecoveryManager::apply_records(&records);

        let engine = VeritasEngine {
            global_version: AtomicU64::new(recovered_version),
            state_store: StateStore::from_map(state_map),
            scope_registry: ScopeRegistry::from_map(scope_map),
            tx_id_counter: AtomicU64::new(max_tx_id + 1),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open(&wal_path).expect("Failed to open WAL file"),
            object_registry: Mutex::new(HashSet::new()),
        };

        if !records.is_empty() {
            println!(
                "[恢复] 从WAL恢复 {} 条记录，当前版本号: {}，下一个事务ID: {}",
                records.len(),
                recovered_version,
                max_tx_id + 1
            );
        }

        for pending in pending_effects {
            println!(
                "[恢复][EFFECT重试] 执行 {}: payload长度={}",
                pending.idempotency_key,
                pending.payload.len()
            );
            let ack = WalEntry::EffectAck {
                tx_id: pending.tx_id,
                idempotency_key: pending.idempotency_key,
            };
            if let Err(e) = engine.wal.append_and_sync(&ack) {
                eprintln!("[WARN] 写入 EffectAck 失败: {}", e);
            }
        }

        engine
    }

    pub fn scope_registry(&self) -> &ScopeRegistry {
        &self.scope_registry
    }

    pub fn init_state(&self, state_id: StateId, initial_value: Vec<u8>) {
        self.state_store.insert(
            state_id,
            StateEntry {
                value: initial_value,
                version: 0,
            },
        );
    }

    pub fn peek_state(&self, state_id: StateId) -> Option<StateEntry> {
        self.state_store.read(state_id)
    }

    pub fn init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) {
        ctx.write_set.push(state_id, value);
    }

    pub fn begin(&self) -> TransactionContext {
        let tx_id = self.tx_id_counter.fetch_add(1, Ordering::SeqCst);
        let snapshot_version = self.global_version.load(Ordering::Acquire);
        TransactionContext::new(tx_id, snapshot_version)
    }

    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        if let Some(written_value) = ctx.write_set.get_latest(state_id) {
            return Ok(written_value.clone());
        }

        let entry = self
            .state_store
            .read(state_id)
            .ok_or(VeritasError::EngineError(format!(
                "State {:?} not found",
                state_id
            )))?;

        if entry.version > ctx.snapshot_version() {
            return Err(VeritasError::Abort(AbortReason::ReadFutureVersion));
        }

        ctx.read_set.states.insert(state_id, entry.version);
        Ok(entry.value.clone())
    }

    pub fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        if !ctx.read_set.states.contains_key(&state_id) {
            if let Some(entry) = self.state_store.read(state_id) {
                ctx.read_set.states.insert(state_id, entry.version);
            }
        }

        ctx.write_set.push(state_id, value);
        Ok(())
    }

    pub fn effect(
        &self,
        ctx: &mut TransactionContext,
        payload: Vec<u8>,
    ) -> Result<String, VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let seq = ctx.effect_queue.len();
        let idempotency_key = format!("{}-{}", ctx.tx_id(), seq);
        ctx.effect_queue.push(PendingEffect {
            idempotency_key: idempotency_key.clone(),
            payload,
        });
        Ok(idempotency_key)
    }

    pub fn commit(&self, ctx: &mut TransactionContext) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        let _lock = self.commit_lock.lock().unwrap();

        self.detect_conflict(ctx)?;
        self.detect_scope_conflict(ctx)?;

        let commit_version = self.global_version.load(Ordering::Acquire) + 1;

        let mut writes_map = HashMap::new();
        for (state_id, value) in ctx.write_set.iter() {
            writes_map.insert(*state_id, value.clone());
        }

        let wal_scope_changes: Vec<WalScopeChange> = ctx
            .scope_write_set
            .iter()
            .map(|c| WalScopeChange {
                scope_id: c.scope_id,
                change_type: c.change_type.clone(),
                state_id: c.state_id,
            })
            .collect();

        let pending_effects = ctx.effect_queue.drain();
        let wal_effects: Vec<WalEffect> = pending_effects
            .iter()
            .map(|e| WalEffect {
                idempotency_key: e.idempotency_key.clone(),
                payload: e.payload.clone(),
            })
            .collect();

        let wal_entry = WalEntry::Commit {
            tx_id: ctx.tx_id(),
            version: commit_version,
            writes: writes_map.into_iter().collect(),
            scope_changes: wal_scope_changes,
            effects: wal_effects,
        };

        self.wal
            .append_and_sync(&wal_entry)
            .map_err(|e| VeritasError::EngineError(format!("WAL write failed: {}", e)))?;
        // P4: 写入 ObjectBirth WAL 条目
        for object_id in &ctx.pending_objects {
            let birth_entry = WalEntry::ObjectBirth {
                tx_id: ctx.tx_id(),
                object_id: *object_id,
            };
            self.wal
                .append_and_sync(&birth_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL ObjectBirth write failed: {}", e)))?;
        }

        for (state_id, value) in ctx.write_set.iter() {
            self.state_store.insert(
                *state_id,
                StateEntry {
                    value: value.clone(),
                    version: commit_version,
                },
            );
        }

        for change in &ctx.scope_write_set {
            match change.change_type {
                ScopeChangeType::Bind => {
                    self.scope_registry
                        .apply_bind(change.scope_id, change.state_id);
                }
                ScopeChangeType::Unbind => {
                    self.scope_registry
                        .apply_unbind(change.scope_id, change.state_id);
                }
            }
        }

        self.global_version.fetch_add(1, Ordering::SeqCst);

        // P4: 固化 Object 到全局注册表
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_objects {
                registry.insert(*object_id);
            }
        }

        drop(_lock);

        for pending in pending_effects {
            println!(
                "[EFFECT] 执行 {}: payload长度={}",
                pending.idempotency_key,
                pending.payload.len()
            );
            let ack = WalEntry::EffectAck {
                tx_id: ctx.tx_id(),
                idempotency_key: pending.idempotency_key.clone(),
            };
            if let Err(e) = self.wal.append_and_sync(&ack) {
                eprintln!("[WARN] 写入 EffectAck 失败: {}", e);
            }
        }

        Ok(())
    }

    /// P4: OBJECT_BIRTH 最小物理原语
    pub fn object_birth(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 检查全局注册表：若已存在则拒绝
        let registry = self.object_registry.lock().unwrap();
        if registry.contains(&object_id) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        drop(registry);

        // 检查当前事务暂存区：防重复
        if ctx.pending_objects.contains(&object_id) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        ctx.pending_objects.push(object_id);
        Ok(())
    }

    pub fn abort(&self, ctx: &mut TransactionContext, _reason: AbortReason) {
        ctx.set_aborted();
    }

    fn detect_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        for (state_id, read_version) in &ctx.read_set.states {
            if let Some(entry) = self.state_store.read(*state_id) {
                if entry.version > *read_version {
                    return Err(AbortReason::WriteConflict);
                }
            }
        }
        Ok(())
    }

    fn detect_scope_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        for (scope_id, read_version) in &ctx.read_set.scopes {
            if let Some(current_version) = self.scope_registry.struct_version(*scope_id) {
                if current_version > *read_version {
                    return Err(AbortReason::PhantomConflict);
                }
            }
        }
        Ok(())
    }

    pub fn get_global_version(&self) -> Version {
        self.global_version.load(Ordering::Acquire)
    }

    pub fn savepoint(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        ctx.savepoints.push(Savepoint {
            name: name.to_string(),
            write_set_len: ctx.write_set.len(),
            effect_queue_len: ctx.effect_queue.len(),
            scope_write_set_len: ctx.scope_write_set.len(),
        });

        Ok(())
    }

    pub fn rollback_to(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        let index = ctx
            .savepoints
            .iter()
            .rposition(|s| s.name == name)
            .ok_or_else(|| {
                VeritasError::EngineError(format!("savepoint '{}' not found", name))
            })?;

        let sp = ctx.savepoints[index].clone();

        ctx.write_set.truncate(sp.write_set_len);
        ctx.effect_queue.truncate(sp.effect_queue_len);
        ctx.scope_write_set.truncate(sp.scope_write_set_len);

        ctx.savepoints.truncate(index + 1);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_wal_path() -> String {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("wal_test_{}_{}.log", std::process::id(), n)
    }

    fn cleanup_wal(path: &str) {
        let _ = fs::remove_file(path);
    }

    fn setup_engine() -> (VeritasEngine, StateId, String) {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine.init_state(state_a, 100u64.to_le_bytes().to_vec());
        (engine, state_a, path)
    }

    #[test]
    fn test_wal_file_created() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine.init_state(state_a, 100u64.to_le_bytes().to_vec());

        let mut ctx = engine.begin();
        engine
            .write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx).unwrap();

        let metadata = fs::metadata(&path);
        assert!(metadata.is_ok());
        assert!(metadata.unwrap().len() > 0);
        cleanup_wal(&path);
    }

    #[test]
    fn test_recovery_after_commit() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine1 = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine1.init_state(state_a, 100u64.to_le_bytes().to_vec());

        let mut ctx = engine1.begin();
        engine1
            .write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine1.commit(&mut ctx).unwrap();
        drop(engine1);

        let engine2 = VeritasEngine::with_wal_path(path.clone());
        let mut ctx2 = engine2.begin();
        let val = engine2.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&val), 50);
        assert!(engine2.get_global_version() >= 1);
        cleanup_wal(&path);
    }

    #[test]
    fn test_multiple_commits_recovery() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine1 = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine1.init_state(state_a, 0u64.to_le_bytes().to_vec());

        for _i in 1..=3 {
            let mut ctx = engine1.begin();
            let val = engine1.read(&mut ctx, state_a).unwrap();
            let current = bytes_to_u64(&val);
            engine1
                .write(
                    &mut ctx,
                    state_a,
                    (current + 100).to_le_bytes().to_vec(),
                )
                .unwrap();
            engine1.commit(&mut ctx).unwrap();
        }
        drop(engine1);

        let engine2 = VeritasEngine::with_wal_path(path.clone());
        let mut ctx2 = engine2.begin();
        let val = engine2.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&val), 300);
        cleanup_wal(&path);
    }

    #[test]
    fn test_empty_wal_recovery() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());
        assert_eq!(engine.get_global_version(), 0);
        cleanup_wal(&path);
    }

    #[test]
    fn test_read_your_writes() {
        let (engine, state_a, path) = setup_engine();
        let mut ctx = engine.begin();
        engine
            .write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        let balance = engine.read(&mut ctx, state_a).unwrap();
        assert_eq!(bytes_to_u64(&balance), 50);
        engine.commit(&mut ctx).unwrap();

        let mut ctx2 = engine.begin();
        let final_val = engine.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&final_val), 50);
        cleanup_wal(&path);
    }

    #[test]
    fn test_blind_write_conflict() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine.init_state(state_a, 100u64.to_le_bytes().to_vec());

        let mut ctx1 = engine.begin();
        engine
            .write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();

        let mut ctx2 = engine.begin();
        engine
            .write(&mut ctx2, state_a, 30u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx2).unwrap();

        let result = engine.commit(&mut ctx1);
        assert!(result.is_err());

        let mut ctx3 = engine.begin();
        let final_val = engine.read(&mut ctx3, state_a).unwrap();
        assert_eq!(bytes_to_u64(&final_val), 30);
        cleanup_wal(&path);
    }

    #[test]
    fn test_basic_transaction() {
        let (engine, state_a, path) = setup_engine();
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
        cleanup_wal(&path);
    }

    #[test]
    fn test_write_conflict() {
        let (engine, state_a, path) = setup_engine();

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
        cleanup_wal(&path);
    }

    #[test]
    fn test_isolation() {
        let (engine, state_a, path) = setup_engine();

        let mut ctx1 = engine.begin();
        engine
            .write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();

        let mut ctx2 = engine.begin();
        let balance = engine.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&balance), 100);
        cleanup_wal(&path);
    }

    #[test]
    fn test_read_future_version_aborts() {
        let (engine, state_a, path) = setup_engine();

        let mut ctx1 = engine.begin();
        let mut ctx2 = engine.begin();
        engine
            .write(&mut ctx2, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine.commit(&mut ctx2).unwrap();

        let result = engine.read(&mut ctx1, state_a);
        assert!(result.is_err());
        cleanup_wal(&path);
    }

    #[test]
    fn test_recovery_then_new_transaction_conflict_detection() {
        let path = test_wal_path();
        cleanup_wal(&path);
        let engine1 = VeritasEngine::with_wal_path(path.clone());
        let state_a = deterministic_hash("Account::A::Balance");
        engine1.init_state(state_a, 100u64.to_le_bytes().to_vec());

        let mut ctx1 = engine1.begin();
        engine1
            .write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec())
            .unwrap();
        engine1.commit(&mut ctx1).unwrap();
        let version_after_commit = engine1.get_global_version();
        drop(engine1);

        let engine2 = VeritasEngine::with_wal_path(path.clone());
        assert_eq!(engine2.get_global_version(), version_after_commit);

        let mut ctx2 = engine2.begin();
        assert_eq!(ctx2.snapshot_version(), version_after_commit);
        let val = engine2.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&val), 50);

        engine2
            .write(&mut ctx2, state_a, 30u64.to_le_bytes().to_vec())
            .unwrap();
        engine2.commit(&mut ctx2).unwrap();
        assert_eq!(engine2.get_global_version(), version_after_commit + 1);

        cleanup_wal(&path);
    }

    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_concurrent_transactions() {
        let path = format!("wal_stress_{}.log", std::process::id());
        let _ = std::fs::remove_file(&path);

        let engine = Arc::new(VeritasEngine::with_wal_path(path.clone()));
        let state_x = deterministic_hash("Stress::Counter::X");
        engine.init_state(state_x, 0u64.to_le_bytes().to_vec());

        const N_THREADS: usize = 12;
        const OPS_PER_THREAD: usize = 50;

        let success_count = Arc::new(AtomicU64::new(0));

        let handles: Vec<_> = (0..N_THREADS)
            .map(|_| {
                let engine = Arc::clone(&engine);
                let success_count = Arc::clone(&success_count);
                thread::spawn(move || {
                    for _ in 0..OPS_PER_THREAD {
                        loop {
                            let mut ctx = engine.begin();
                            let val = match engine.read(&mut ctx, state_x) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let current = bytes_to_u64(&val);
                            engine
                                .write(
                                    &mut ctx,
                                    state_x,
                                    (current + 1).to_le_bytes().to_vec(),
                                )
                                .unwrap();

                            match engine.commit(&mut ctx) {
                                Ok(()) => {
                                    success_count.fetch_add(1, Ordering::SeqCst);
                                    break;
                                }
                                Err(_) => continue,
                            }
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let expected = success_count.load(Ordering::SeqCst);
        let mut ctx = engine.begin();
        let final_val = engine.read(&mut ctx, state_x).unwrap();
        let final_count = bytes_to_u64(&final_val);
        assert_eq!(final_count, expected);
        assert_eq!(expected, (N_THREADS * OPS_PER_THREAD) as u64);
        assert_eq!(engine.get_global_version(), expected);

        drop(engine);

        let engine2 = VeritasEngine::with_wal_path(path.clone());
        let mut ctx2 = engine2.begin();
        let recovered_val = engine2.read(&mut ctx2, state_x).unwrap();
        let recovered_count = bytes_to_u64(&recovered_val);
        assert_eq!(recovered_count, expected);
        assert_eq!(engine2.get_global_version(), expected);

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(test)]
    mod scope_tests {
        use super::*;
        use crate::scope::ScopeExt;

        #[test]
        fn test_family_shared_limit_phantom_read_protection() {
            let path = format!("wal_scope_test_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());

            let account_a = deterministic_hash("Family::AccountA::Used");
            let account_b = deterministic_hash("Family::AccountB::Used");
            engine.init_state(account_a, 0u64.to_le_bytes().to_vec());
            engine.init_state(account_b, 0u64.to_le_bytes().to_vec());
            engine.declare_scope("family_group_1");

            const LIMIT: u64 = 10000;

            let mut tx1 = engine.begin();
            engine
                .enumerate_scope(&mut tx1, "family_group_1")
                .unwrap();
            let used_a =
                bytes_to_u64(&engine.read(&mut tx1, account_a).unwrap()[..8]);
            let used_b =
                bytes_to_u64(&engine.read(&mut tx1, account_b).unwrap()[..8]);
            assert!(used_a + used_b + 6000 <= LIMIT);
            engine
                .write(&mut tx1, account_a, (used_a + 6000).to_le_bytes().to_vec())
                .unwrap();

            let mut tx2 = engine.begin();
            let account_c = deterministic_hash("Family::AccountC::Used");
            engine.init_state_in_tx(&mut tx2, account_c, 5000u64.to_le_bytes().to_vec());
            engine
                .bind_scope(&mut tx2, "family_group_1", account_c)
                .unwrap();
            engine.commit(&mut tx2).unwrap();
            println!("[事务2] 添加成员C(初始额度5000) COMMIT 成功");

            let result = engine.commit(&mut tx1);
            assert!(result.is_err());
            println!(
                "[事务1] 提现6000 COMMIT 失败: {:?} ✓ Scope幻读保护生效",
                result
            );

            let _ = std::fs::remove_file(&path);
        }
    }

    #[cfg(test)]
    mod scope_regression_tests {
        use super::*;
        use crate::scope::ScopeExt;

        #[test]
        fn test_scope_read_only_does_not_cause_false_conflict() {
            let path = format!("wal_scope_regress_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());

            let account_a = deterministic_hash("Regress::AccountA::Used");
            let account_b = deterministic_hash("Regress::AccountB::Used");
            engine.init_state(account_a, 0u64.to_le_bytes().to_vec());
            engine.init_state(account_b, 0u64.to_le_bytes().to_vec());
            engine.declare_scope("family_group_regress");

            let mut tx1 = engine.begin();
            engine
                .enumerate_scope(&mut tx1, "family_group_regress")
                .unwrap();
            let used_a =
                bytes_to_u64(&engine.read(&mut tx1, account_a).unwrap()[..8]);
            engine
                .write(&mut tx1, account_a, (used_a + 2000).to_le_bytes().to_vec())
                .unwrap();

            let mut tx2 = engine.begin();
            engine
                .enumerate_scope(&mut tx2, "family_group_regress")
                .unwrap();
            let used_b =
                bytes_to_u64(&engine.read(&mut tx2, account_b).unwrap()[..8]);
            engine
                .write(&mut tx2, account_b, (used_b + 3000).to_le_bytes().to_vec())
                .unwrap();
            engine.commit(&mut tx2).unwrap();

            let result = engine.commit(&mut tx1);
            assert!(result.is_ok());

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_scope_bind_rolls_back_with_savepoint() {
            let path = format!("wal_scope_rollback_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());

            let member = deterministic_hash("Scope::Member::X");
            engine.declare_scope("rollback_test_scope");

            let mut tx = engine.begin();
            engine.savepoint(&mut tx, "before_bind").unwrap();
            engine.init_state_in_tx(&mut tx, member, 1u64.to_le_bytes().to_vec());
            engine
                .bind_scope(&mut tx, "rollback_test_scope", member)
                .unwrap();

            engine.rollback_to(&mut tx, "before_bind").unwrap();
            engine.commit(&mut tx).unwrap();

            let id = crate::scope::scope_state_id("rollback_test_scope");
            assert_eq!(engine.scope_registry().struct_version(id), Some(0));

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_scope_struct_version_survives_restart() {
            let path = format!("wal_scope_persist_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine1 = VeritasEngine::with_wal_path(path.clone());
            engine1.declare_scope("persist_test_scope");
            let member = deterministic_hash("Persist::Member");

            let mut tx = engine1.begin();
            engine1.init_state_in_tx(&mut tx, member, 1u64.to_le_bytes().to_vec());
            engine1
                .bind_scope(&mut tx, "persist_test_scope", member)
                .unwrap();
            engine1.commit(&mut tx).unwrap();
            drop(engine1);

            let engine2 = VeritasEngine::with_wal_path(path.clone());
            let id = crate::scope::scope_state_id("persist_test_scope");
            assert_eq!(engine2.scope_registry().struct_version(id), Some(1));

            let _ = std::fs::remove_file(&path);
        }
    }

    #[cfg(test)]
    mod crash_recovery_tests {
        use super::*;

        #[test]
        fn test_crash_recovery_retries_unacked_effect() {
            let path = format!("wal_effect_crash_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);

            {
                let writer = crate::wal::WalWriter::open(&path).unwrap();
                let commit = crate::wal::WalEntry::Commit {
                    tx_id: 1,
                    version: 1,
                    writes: vec![(
                        deterministic_hash("CrashTest::A"),
                        1u64.to_le_bytes().to_vec(),
                    )],
                    scope_changes: vec![],
                    effects: vec![crate::wal::WalEffect {
                        idempotency_key: "1-0".to_string(),
                        payload: b"pending notification".to_vec(),
                    }],
                };
                writer.append_and_sync(&commit).unwrap();
            }

            let engine = VeritasEngine::with_wal_path(path.clone());
            assert_eq!(engine.get_global_version(), 1);

            let (records, _) = crate::wal::RecoveryManager::recover(&path).unwrap();
            let ack_count = records
                .iter()
                .filter(|r| matches!(
                    r,
                    crate::wal::WalEntry::EffectAck { idempotency_key, .. }
                    if idempotency_key == "1-0"
                ))
                .count();
            assert_eq!(ack_count, 1, "重启后应补写一条 EffectAck");

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_tx_id_counter_survives_restart() {
            let path = format!("wal_txid_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine1 = VeritasEngine::with_wal_path(path.clone());
            let state = deterministic_hash("TxId::Continuity");
            engine1.init_state(state, 0u64.to_le_bytes().to_vec());

            let mut ctx = engine1.begin();
            let last_tx_id = ctx.tx_id();
            engine1
                .write(&mut ctx, state, 1u64.to_le_bytes().to_vec())
                .unwrap();
            engine1.commit(&mut ctx).unwrap();
            drop(engine1);

            let engine2 = VeritasEngine::with_wal_path(path.clone());
            let ctx2 = engine2.begin();
            assert!(ctx2.tx_id() > last_tx_id, "重启后 tx_id 不应该撞车");

            let _ = std::fs::remove_file(&path);
        }
    }

    #[cfg(test)]
    mod effect_tests {
        use super::*;

        #[test]
        fn test_effect_not_executed_on_abort() {
            let path = format!("wal_effect_abort_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state_a = deterministic_hash("Effect::A");
            engine.init_state(state_a, 0u64.to_le_bytes().to_vec());

            let mut tx1 = engine.begin();
            engine
                .write(&mut tx1, state_a, 10u64.to_le_bytes().to_vec())
                .unwrap();

            let mut tx2 = engine.begin();
            let _key = engine
                .effect(
                    &mut tx2,
                    b"notification: should not execute".to_vec(),
                )
                .unwrap();
            let _ = engine.read(&mut tx2, state_a).unwrap();
            engine
                .write(&mut tx2, state_a, 20u64.to_le_bytes().to_vec())
                .unwrap();

            engine.commit(&mut tx1).unwrap();
            let result = engine.commit(&mut tx2);

            assert!(result.is_err());
            let mut ctx_check = engine.begin();
            let val = engine.read(&mut ctx_check, state_a).unwrap();
            assert_eq!(bytes_to_u64(&val), 10);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_effect_executed_after_commit() {
            let path = format!("wal_effect_commit_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state_a = deterministic_hash("Effect::A");
            engine.init_state(state_a, 0u64.to_le_bytes().to_vec());

            let mut ctx = engine.begin();
            let key = engine
                .effect(&mut ctx, b"notification: send email".to_vec())
                .unwrap();
            engine
                .write(&mut ctx, state_a, 100u64.to_le_bytes().to_vec())
                .unwrap();
            engine.commit(&mut ctx).unwrap();

            let mut ctx_check = engine.begin();
            let val = engine.read(&mut ctx_check, state_a).unwrap();
            assert_eq!(bytes_to_u64(&val), 100);

            assert!(key.starts_with("1-"));
            let _ = std::fs::remove_file(&path);
        }
    }

    #[cfg(test)]
    mod savepoint_tests {
        use super::*;

        #[test]
        fn test_savepoint_basic() {
            let path = format!("wal_savepoint_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state = deterministic_hash("Savepoint::Counter");
            engine.init_state(state, 0u64.to_le_bytes().to_vec());

            let mut tx = engine.begin();

            engine
                .write(&mut tx, state, 10u64.to_le_bytes().to_vec())
                .unwrap();
            engine.savepoint(&mut tx, "sp1").unwrap();

            engine
                .write(&mut tx, state, 20u64.to_le_bytes().to_vec())
                .unwrap();
            engine.savepoint(&mut tx, "sp2").unwrap();

            engine
                .write(&mut tx, state, 30u64.to_le_bytes().to_vec())
                .unwrap();

            engine.rollback_to(&mut tx, "sp2").unwrap();
            engine.commit(&mut tx).unwrap();

            let mut check = engine.begin();
            let val = u64::from_le_bytes(
                engine.read(&mut check, state).unwrap()[..8]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(val, 20);

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_savepoint_nested() {
            let path = format!("wal_savepoint_nested_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state = deterministic_hash("Savepoint::Nested");
            engine.init_state(state, 0u64.to_le_bytes().to_vec());

            let mut tx = engine.begin();

            engine
                .write(&mut tx, state, 10u64.to_le_bytes().to_vec())
                .unwrap();
            engine.savepoint(&mut tx, "outer").unwrap();

            engine
                .write(&mut tx, state, 20u64.to_le_bytes().to_vec())
                .unwrap();
            engine.savepoint(&mut tx, "inner").unwrap();

            engine
                .write(&mut tx, state, 30u64.to_le_bytes().to_vec())
                .unwrap();

            engine.rollback_to(&mut tx, "inner").unwrap();
            engine.rollback_to(&mut tx, "outer").unwrap();

            engine.commit(&mut tx).unwrap();

            let mut check = engine.begin();
            let val = u64::from_le_bytes(
                engine.read(&mut check, state).unwrap()[..8]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(val, 10);

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_savepoint_effect_rollback() {
            let path = format!("wal_savepoint_effect_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state = deterministic_hash("Savepoint::Effect");
            engine.init_state(state, 0u64.to_le_bytes().to_vec());

            let mut tx = engine.begin();

            engine
                .write(&mut tx, state, 10u64.to_le_bytes().to_vec())
                .unwrap();
            let _key1 = engine.effect(&mut tx, b"effect 1".to_vec()).unwrap();

            engine.savepoint(&mut tx, "sp1").unwrap();

            engine
                .write(&mut tx, state, 20u64.to_le_bytes().to_vec())
                .unwrap();
            let _key2 = engine.effect(&mut tx, b"effect 2".to_vec()).unwrap();

            engine.rollback_to(&mut tx, "sp1").unwrap();

            engine.commit(&mut tx).unwrap();

            let mut check = engine.begin();
            let val = u64::from_le_bytes(
                engine.read(&mut check, state).unwrap()[..8]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(val, 10);

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_savepoint_not_found() {
            let path = format!("wal_savepoint_notfound_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state = deterministic_hash("Savepoint::NotFound");
            engine.init_state(state, 0u64.to_le_bytes().to_vec());

            let mut tx = engine.begin();

            let result = engine.rollback_to(&mut tx, "nonexistent");
            assert!(result.is_err());

            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn test_savepoint_multiple_states() {
            let path = format!("wal_savepoint_multi_{}.log", std::process::id());
            let _ = std::fs::remove_file(&path);
            let engine = VeritasEngine::with_wal_path(path.clone());
            let state_a = deterministic_hash("Savepoint::A");
            let state_b = deterministic_hash("Savepoint::B");
            engine.init_state(state_a, 0u64.to_le_bytes().to_vec());
            engine.init_state(state_b, 0u64.to_le_bytes().to_vec());

            let mut tx = engine.begin();

            engine
                .write(&mut tx, state_a, 10u64.to_le_bytes().to_vec())
                .unwrap();
            engine
                .write(&mut tx, state_b, 20u64.to_le_bytes().to_vec())
                .unwrap();
            engine.savepoint(&mut tx, "sp1").unwrap();

            engine
                .write(&mut tx, state_a, 30u64.to_le_bytes().to_vec())
                .unwrap();
            engine
                .write(&mut tx, state_b, 40u64.to_le_bytes().to_vec())
                .unwrap();

            engine.rollback_to(&mut tx, "sp1").unwrap();
            engine.commit(&mut tx).unwrap();

            let mut check = engine.begin();
            let val_a = u64::from_le_bytes(
                engine.read(&mut check, state_a).unwrap()[..8]
                    .try_into()
                    .unwrap(),
            );
            let val_b = u64::from_le_bytes(
                engine.read(&mut check, state_b).unwrap()[..8]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(val_a, 10);
            assert_eq!(val_b, 20);

            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn test_object_birth_commit_and_abort() {
        use std::sync::Mutex;
        let engine = VeritasEngine::new();

        // 测试1: commit 后 Object 存在
        let mut tx = engine.begin();
        let obj_id: ObjectId = 101;
        engine.object_birth(&mut tx, obj_id).unwrap();
        engine.commit(&mut tx).unwrap();

        let registry = engine.object_registry.lock().unwrap();
        assert!(registry.contains(&obj_id), "commit 后注册表应包含 Object");
        drop(registry);

        // 测试2: abort 后 Object 不存在
        let mut tx2 = engine.begin();
        let obj_id2: ObjectId = 102;
        engine.object_birth(&mut tx2, obj_id2).unwrap();
        engine.abort(&mut tx2, AbortReason::WriteConflict);

        let registry2 = engine.object_registry.lock().unwrap();
        drop(registry2);

        // 测试3: 重复 ObjectId 应拒绝
        let mut tx3 = engine.begin();
        let result = engine.object_birth(&mut tx3, obj_id); // 101 已存在
        assert!(result.is_err(), "重复 ObjectId 应报错");
        engine.abort(&mut tx3, AbortReason::WriteConflict);
    }
}
