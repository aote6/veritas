// Veritas Kernel V0.2 - 事务引擎核心

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::types::*;
use crate::wal::{RecoveryManager, WalRecord, WalWriter};
use crate::transaction::TransactionContext;
use crate::store::StateStore;

fn bytes_to_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}

const WAL_PATH: &str = "wal.log";

pub struct VeritasEngine {
    global_version: AtomicU64,
    state_store: StateStore,
    tx_id_counter: AtomicU64,
    commit_lock: Mutex<()>,
    wal: WalWriter,
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

        let mut state_map = HashMap::new();
        RecoveryManager::apply_records(&records, &mut state_map);

        let engine = VeritasEngine {
            global_version: AtomicU64::new(recovered_version),
            state_store: StateStore::from_map(state_map),
            tx_id_counter: AtomicU64::new(1),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open(&wal_path).expect("Failed to open WAL file"),
        };

        if !records.is_empty() {
            println!("[恢复] 从WAL恢复 {} 条记录，当前版本号: {}", records.len(), recovered_version);
        }

        engine
    }

    pub fn init_state(&self, state_id: StateId, initial_value: Vec<u8>) {
        self.state_store.insert(state_id, StateEntry {
            value: initial_value,
            version: 0,
        });
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
        ctx.write_set.state_changes.insert(state_id, value);
    }

    pub fn begin(&self) -> TransactionContext {
        let tx_id = self.tx_id_counter.fetch_add(1, Ordering::SeqCst);
        let snapshot_version = self.global_version.load(Ordering::Acquire);
        TransactionContext::new(tx_id, snapshot_version)
    }

    pub fn read(&self, ctx: &mut TransactionContext, state_id: StateId) -> Result<Vec<u8>, VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        if let Some(written_value) = ctx.write_set.state_changes.get(&state_id) {
            return Ok(written_value.clone());
        }

        let entry = self.state_store.read(state_id)
            .ok_or(VeritasError::EngineError(format!("State {:?} not found", state_id)))?;

        if entry.version > ctx.snapshot_version() {
            return Err(VeritasError::Abort(AbortReason::ReadFutureVersion));
        }

        ctx.read_set.states.insert(state_id, entry.version);
        Ok(entry.value.clone())
    }

    pub fn write(&self, ctx: &mut TransactionContext, state_id: StateId, value: Vec<u8>) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        if !ctx.read_set.states.contains_key(&state_id) {
            if let Some(entry) = self.state_store.read(state_id) {
                ctx.read_set.states.insert(state_id, entry.version);
            }
        }

        ctx.write_set.state_changes.insert(state_id, value);
        Ok(())
    }

    pub fn effect(&self, ctx: &mut TransactionContext, payload: Vec<u8>) -> Result<String, VeritasError> {
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

        let commit_version = self.global_version.load(Ordering::Acquire) + 1;
        let wal_record = WalRecord {
            tx_id: ctx.tx_id(),
            version: commit_version,
            writes: ctx.write_set.state_changes.iter()
                .map(|(id, val)| (*id, val.clone()))
                .collect(),
        };

        self.wal.append_and_sync(&wal_record)
            .map_err(|e| VeritasError::EngineError(format!("WAL write failed: {}", e)))?;

        for (state_id, new_value) in &ctx.write_set.state_changes {
            self.state_store.insert(*state_id, StateEntry {
                value: new_value.clone(),
                version: commit_version,
            });
        }

        self.global_version.fetch_add(1, Ordering::SeqCst);
        drop(_lock);

        for pending in ctx.effect_queue.drain() {
            println!("[EFFECT] 执行 {}: payload长度={}", pending.idempotency_key, pending.payload.len());
        }

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

    pub fn get_global_version(&self) -> Version {
        self.global_version.load(Ordering::Acquire)
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
        engine.write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
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
        engine1.write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
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
            engine1.write(&mut ctx, state_a, (current + 100).to_le_bytes().to_vec()).unwrap();
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
        engine.write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
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
        engine.write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec()).unwrap();

        let mut ctx2 = engine.begin();
        engine.write(&mut ctx2, state_a, 30u64.to_le_bytes().to_vec()).unwrap();
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
        engine.write(&mut ctx, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
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
        engine.write(&mut ctx2, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
        engine.commit(&mut ctx2).unwrap();

        engine.write(&mut ctx1, state_a, 30u64.to_le_bytes().to_vec()).unwrap();
        let result = engine.commit(&mut ctx1);
        assert!(result.is_err());
        cleanup_wal(&path);
    }

    #[test]
    fn test_isolation() {
        let (engine, state_a, path) = setup_engine();

        let mut ctx1 = engine.begin();
        engine.write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec()).unwrap();

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
        engine.write(&mut ctx2, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
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
        engine1.write(&mut ctx1, state_a, 50u64.to_le_bytes().to_vec()).unwrap();
        engine1.commit(&mut ctx1).unwrap();
        let version_after_commit = engine1.get_global_version();
        drop(engine1);

        let engine2 = VeritasEngine::with_wal_path(path.clone());
        assert_eq!(engine2.get_global_version(), version_after_commit);

        let mut ctx2 = engine2.begin();
        assert_eq!(ctx2.snapshot_version(), version_after_commit);
        let val = engine2.read(&mut ctx2, state_a).unwrap();
        assert_eq!(bytes_to_u64(&val), 50);

        engine2.write(&mut ctx2, state_a, 30u64.to_le_bytes().to_vec()).unwrap();
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
                                .write(&mut ctx, state_x, (current + 1).to_le_bytes().to_vec())
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
            engine.touch_scope_read(&mut tx1, "family_group_1").unwrap();
            let used_a = bytes_to_u64(
                &engine.read(&mut tx1, account_a).unwrap()[..8],
            );
            let used_b = bytes_to_u64(
                &engine.read(&mut tx1, account_b).unwrap()[..8],
            );
            assert!(used_a + used_b + 6000 <= LIMIT);
            engine
                .write(&mut tx1, account_a, (used_a + 6000).to_le_bytes().to_vec())
                .unwrap();

            let mut tx2 = engine.begin();
            let account_c = deterministic_hash("Family::AccountC::Used");
            engine.touch_scope_write(&mut tx2, "family_group_1").unwrap();
            engine.init_state_in_tx(&mut tx2, account_c, 5000u64.to_le_bytes().to_vec());
            engine.commit(&mut tx2).unwrap();
            println!("[事务2] 添加成员C(初始额度5000) COMMIT 成功");

            let result = engine.commit(&mut tx1);
            assert!(result.is_err());
            println!("[事务1] 提现6000 COMMIT 失败: {:?} ✓ Scope幻读保护生效", result);

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
            engine.touch_scope_read(&mut tx1, "family_group_regress").unwrap();
            let used_a = bytes_to_u64(
                &engine.read(&mut tx1, account_a).unwrap()[..8],
            );
            engine
                .write(&mut tx1, account_a, (used_a + 2000).to_le_bytes().to_vec())
                .unwrap();

            let mut tx2 = engine.begin();
            engine.touch_scope_read(&mut tx2, "family_group_regress").unwrap();
            let used_b = bytes_to_u64(
                &engine.read(&mut tx2, account_b).unwrap()[..8],
            );
            engine
                .write(&mut tx2, account_b, (used_b + 3000).to_le_bytes().to_vec())
                .unwrap();
            engine.commit(&mut tx2).unwrap();

            let result = engine.commit(&mut tx1);
            assert!(result.is_ok());

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
            engine.write(&mut tx1, state_a, 10u64.to_le_bytes().to_vec()).unwrap();

            let mut tx2 = engine.begin();
            let _key = engine.effect(&mut tx2, b"notification: should not execute".to_vec()).unwrap();
            let _ = engine.read(&mut tx2, state_a).unwrap();
            engine.write(&mut tx2, state_a, 20u64.to_le_bytes().to_vec()).unwrap();

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
            let key = engine.effect(&mut ctx, b"notification: send email".to_vec()).unwrap();
            engine.write(&mut ctx, state_a, 100u64.to_le_bytes().to_vec()).unwrap();
            engine.commit(&mut ctx).unwrap();

            let mut ctx_check = engine.begin();
            let val = engine.read(&mut ctx_check, state_a).unwrap();
            assert_eq!(bytes_to_u64(&val), 100);

            assert!(key.starts_with("1-"));
            let _ = std::fs::remove_file(&path);
        }
    }
}
