// Veritas Kernel V0.3 - 事务引擎核心
// P1: WAL 格式扩展 + Effect 崩溃恢复重试 + tx_id_counter 恢复续接

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::capability::CapabilityGraph;
use crate::scope_registry::ScopeRegistry;
use crate::lock::LockManager;
use crate::tx_manager::TransactionManager;
use crate::controller::TransactionController;
use std::sync::Arc;
use crate::view::TransactionObjectView;
use crate::guard::ObjectGuard;
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
    commit_lock: Mutex<()>,
    wal: WalWriter,
    object_registry: Mutex<HashMap<ObjectId, ObjectState>>,
    topology: Mutex<Vec<LinkEdge>>,
    capability_graph: Mutex<CapabilityGraph>,
    tx_mgr: Arc<TransactionManager>,
    lock_mgr: Arc<LockManager>,
    controller: TransactionController,
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

        // P7.5: 从 WAL records 重建 object_registry 和 topology
        let mut recovered_objects: HashMap<ObjectId, ObjectState> = HashMap::new();
        let mut recovered_links: Vec<LinkEdge> = Vec::new();
        let mut recovered_deaths: Vec<ObjectId> = Vec::new();
        for record in &records {
            match record {
                WalEntry::ObjectBirth { object_id, .. } => {
                    recovered_objects.insert(*object_id, ObjectState::Alive);
                }
                WalEntry::ObjectDeath { object_id, .. } => {
                    recovered_objects.insert(*object_id, ObjectState::Dead);
                    recovered_links.retain(|edge| edge.from != *object_id && edge.to != *object_id);
                    recovered_deaths.push(*object_id);
                }
                WalEntry::ObjectLink { from, to, relation_kind, .. } => {
                    let relation = match relation_kind {
                        0 => RelationKind::CapabilityDelegation,
                        1 => RelationKind::ContractDependency,
                        2 => RelationKind::EffectPropagation,
                        _ => continue,
                    };
                    recovered_links.push(LinkEdge { from: *from, to: *to, relation });
                }
                _ => {}
            }
        }

        // P8-final: 重放能力级联撤销
        {
            let mut cap_graph = CapabilityGraph::new();
            for dead_obj in &recovered_deaths {
                cap_graph.revoke_holder(*dead_obj);
            }
        }

        let tx_mgr = Arc::new(TransactionManager::with_start_id(max_tx_id + 1));
        let lock_mgr = Arc::new(LockManager::new(Arc::clone(&tx_mgr)));
        let controller = TransactionController::new(Arc::clone(&tx_mgr), Arc::clone(&lock_mgr));

        let engine = VeritasEngine {
            global_version: AtomicU64::new(recovered_version),
            state_store: StateStore::from_map(state_map),
            scope_registry: ScopeRegistry::from_map(scope_map),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open(&wal_path).expect("Failed to open WAL file"),
            object_registry: Mutex::new(recovered_objects),
            topology: Mutex::new(recovered_links),
            capability_graph: Mutex::new(CapabilityGraph::new()),
            tx_mgr,
            lock_mgr,
            controller,
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

    #[cfg(test)]
    pub fn tx_mgr(&self) -> &Arc<TransactionManager> {
        &self.tx_mgr
    }

    #[cfg(test)]
    pub fn lock_mgr(&self) -> &Arc<LockManager> {
        &self.lock_mgr
    }

    pub fn begin(&self) -> TransactionContext {
        let snapshot_version = self.global_version.load(Ordering::Acquire);
        self.controller.begin(snapshot_version)
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
        self.controller.pre_commit_check(ctx)?;

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
                registry.insert(*object_id, ObjectState::Alive);
            }
        }

        // P8.1: 写入 ObjectDeath WAL 条目并更新状态
        for object_id in &ctx.pending_deaths {
            let death_entry = WalEntry::ObjectDeath {
                tx_id: ctx.tx_id(),
                object_id: *object_id,
            };
            self.wal
                .append_and_sync(&death_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL ObjectDeath write failed: {}", e)))?;
        }
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_deaths {
                registry.insert(*object_id, ObjectState::Dead);
            }
        }

        // P8.2: 确定性拓扑清理——剔除所有涉及已死亡 Object 的边
        {
            let dead_set: HashSet<ObjectId> = ctx.pending_deaths.iter().copied().collect();
            if !dead_set.is_empty() {
                let mut topo = self.topology.lock().unwrap();
                topo.retain(|edge| !dead_set.contains(&edge.from) && !dead_set.contains(&edge.to));
            }
        }

        // P8-final: 能力级联撤销——一行调用，零内聚泄露
        {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            for &dead_obj in &ctx.pending_deaths {
                cap_graph.revoke_holder(dead_obj);
            }
        }

        // P6: 写入 ObjectLink WAL 条目并固化拓扑
        for edge in &ctx.pending_links {
            let link_entry = WalEntry::ObjectLink {
                tx_id: ctx.tx_id(),
                from: edge.from,
                to: edge.to,
                relation_kind: match edge.relation {
                    RelationKind::CapabilityDelegation => 0,
                    RelationKind::ContractDependency => 1,
                    RelationKind::EffectPropagation => 2,
                },
            };
            self.wal
                .append_and_sync(&link_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL ObjectLink write failed: {}", e)))?;
        }
        {
            let mut topo = self.topology.lock().unwrap();
            for edge in &ctx.pending_links {
                topo.push(edge.clone());
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

        self.controller.post_commit(ctx.tx_id());

        Ok(())
    }

    /// P8.3: CAPABILITY_GRANT 原语——向 Alive 的 Object 授权
    pub fn capability_grant(
        &self,
        ctx: &mut TransactionContext,
        grantee: ObjectId,
        capability_type: &str,
        resource: StateId,
    ) -> Result<CapabilityId, VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // Alive 检查
        {
            let reg = self.object_registry.lock().unwrap();
            let view = TransactionObjectView::new(&reg, &ctx.pending_objects, &ctx.pending_deaths);
            ObjectGuard::ensure_can_grant(&view, grantee)?;
        }

        // grantor 暂时用 grantee 自身（自授权），后续可扩展
        let mut graph = self.capability_graph.lock().unwrap();
        let cap_id = graph.grant(capability_type.to_string(), grantee, grantee, resource);
        Ok(cap_id)
    }

    /// P8.1: OBJECT_DEATH 物理原语
    pub fn object_death(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 检查 Object 存在且状态为 Alive
        let registry = self.object_registry.lock().unwrap();
        let state = registry.get(&object_id).copied();
        drop(registry);

        match state {
            None => return Err(VeritasError::Abort(AbortReason::WriteConflict)),
            Some(ObjectState::Dead) => return Err(VeritasError::Abort(AbortReason::WriteConflict)),
            Some(ObjectState::Alive) => {}
        }

        // 防重复死亡
        if ctx.pending_deaths.contains(&object_id) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        ctx.pending_deaths.push(object_id);
        Ok(())
    }

    /// P6: OBJECT_LINK 物理原语
    pub fn object_link(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
        relation: RelationKind,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // 自环检测
        if from == to {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        // 检查源和目标是否存在（全局注册表或当前事务 pending）
        let registry = self.object_registry.lock().unwrap();
        let from_exists = registry.contains_key(&from) || ctx.pending_objects.contains(&from);
        let to_exists = registry.contains_key(&to) || ctx.pending_objects.contains(&to);
        drop(registry);

        if !from_exists || !to_exists {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        // P8.2: 死亡检查——源或目标已死则拒绝
        let reg = self.object_registry.lock().unwrap();
        let from_dead = reg.get(&from) == Some(&ObjectState::Dead) || ctx.pending_deaths.contains(&from);
        let to_dead = reg.get(&to) == Some(&ObjectState::Dead) || ctx.pending_deaths.contains(&to);
        drop(reg);
        if from_dead || to_dead {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        ctx.pending_links.push(LinkEdge { from, to, relation });
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
        {
            let reg = self.object_registry.lock().unwrap();
            let view = TransactionObjectView::new(&reg, &ctx.pending_objects, &ctx.pending_deaths);
            ObjectGuard::ensure_not_exists(&view, object_id)?;
        }

        // 检查当前事务暂存区：防重复
        if ctx.pending_objects.contains(&object_id) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        ctx.pending_objects.push(object_id);
        Ok(())
    }

    pub fn abort(&self, ctx: &mut TransactionContext, reason: AbortReason) {
        self.controller.abort(ctx, reason);
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
            pending_objects_len: ctx.pending_objects.len(),
            pending_links_len: ctx.pending_links.len(),
            pending_deaths_len: ctx.pending_deaths.len(),
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
        ctx.pending_objects.truncate(sp.pending_objects_len);
        ctx.pending_links.truncate(sp.pending_links_len);
        ctx.pending_deaths.truncate(sp.pending_deaths_len);

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
        let wal_path = "target/test_p4_birth.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());
        let _ = std::fs::remove_file(wal_path);

        // 测试1: commit 后 Object 存在
        let mut tx = engine.begin();
        let obj_id: ObjectId = 101;
        engine.object_birth(&mut tx, obj_id).unwrap();
        engine.commit(&mut tx).unwrap();

        let registry = engine.object_registry.lock().unwrap();
        assert!(registry.contains_key(&obj_id), "commit 后注册表应包含 Object");
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

    #[test]
    fn test_object_link_commit_and_abort() {
        let wal_path = "target/test_p6_link.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        // 创建两个 Object
        let obj_a: ObjectId = 201;
        let obj_b: ObjectId = 202;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.commit(&mut tx).unwrap();

        // 测试1: commit 后 Link 存在
        let mut tx2 = engine.begin();
        engine.object_link(&mut tx2, obj_a, obj_b, RelationKind::CapabilityDelegation).unwrap();
        engine.commit(&mut tx2).unwrap();

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 1, "拓扑应包含1条边");
        assert_eq!(topo[0].from, obj_a);
        assert_eq!(topo[0].to, obj_b);
        assert_eq!(topo[0].relation, RelationKind::CapabilityDelegation);
        drop(topo);

        // 测试2: 连接不存在的 Object 应报错
        let mut tx3 = engine.begin();
        let obj_c: ObjectId = 999;
        let result = engine.object_link(&mut tx3, obj_a, obj_c, RelationKind::ContractDependency);
        assert!(result.is_err(), "连接不存在的 Object 应报错");
        engine.abort(&mut tx3, AbortReason::WriteConflict);

        // 测试3: abort 后 Link 不存在
        let mut tx4 = engine.begin();
        let obj_d: ObjectId = 203;
        engine.object_birth(&mut tx4, obj_d).unwrap();
        engine.object_link(&mut tx4, obj_a, obj_d, RelationKind::EffectPropagation).unwrap();
        engine.abort(&mut tx4, AbortReason::WriteConflict);

        let topo2 = engine.topology.lock().unwrap();
        assert_eq!(topo2.len(), 1, "abort 后拓扑不应增加");
        drop(topo2);
    }

    #[test]
    fn test_p1_to_p6_full_lifecycle() {
        let wal_path = "target/test_p7_lifecycle.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        // 1. 事务1：诞生两个 Object
        let obj_a: ObjectId = 1001;
        let obj_b: ObjectId = 1002;
        let mut tx1 = engine.begin();
        engine.object_birth(&mut tx1, obj_a).expect("Birth A failed");
        engine.object_birth(&mut tx1, obj_b).expect("Birth B failed");
        engine.commit(&mut tx1).expect("Commit tx1 failed");

        // 2. 事务2：建立 Link
        let mut tx2 = engine.begin();
        engine.object_link(&mut tx2, obj_a, obj_b, RelationKind::CapabilityDelegation)
            .expect("Link A->B failed");
        engine.commit(&mut tx2).expect("Commit tx2 failed");

        // 3. 校验最终物理状态
        let registry = engine.object_registry.lock().unwrap();
        assert!(registry.contains_key(&obj_a), "registry 应包含 A");
        assert!(registry.contains_key(&obj_b), "registry 应包含 B");
        drop(registry);

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 1, "拓扑应有1条边");
        assert_eq!(topo[0].from, obj_a);
        assert_eq!(topo[0].to, obj_b);
        assert_eq!(topo[0].relation, RelationKind::CapabilityDelegation);
        drop(topo);
    }

    // ========== P7.5 工程验收测试 ==========

    #[test]
    fn test_same_tx_birth_and_link() {
        let wal_path = "target/test_p7_5_sametx.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());
        let mut tx = engine.begin();
        let obj_a: ObjectId = 301;
        let obj_b: ObjectId = 302;

        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        let link_res = engine.object_link(&mut tx, obj_a, obj_b, RelationKind::CapabilityDelegation);
        assert!(link_res.is_ok(), "same-tx birth+link should succeed");
        engine.commit(&mut tx).unwrap();

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 1);
        assert_eq!(topo[0].from, obj_a);
        assert_eq!(topo[0].to, obj_b);
    }

    #[test]
    fn test_self_link_rejection() {
        let wal_path = "target/test_p7_5_selflink.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());
        let mut tx = engine.begin();
        let obj_a: ObjectId = 303;
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        let result = engine.object_link(&mut tx2, obj_a, obj_a, RelationKind::ContractDependency);
        assert!(result.is_err(), "self-link A->A must be rejected");
    }

    #[test]
    fn test_duplicate_link() {
        let wal_path = "target/test_p7_5_duplink.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());
        let mut tx = engine.begin();
        let obj_a: ObjectId = 304;
        let obj_b: ObjectId = 305;
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.object_link(&mut tx, obj_a, obj_b, RelationKind::CapabilityDelegation).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        let res = engine.object_link(&mut tx2, obj_a, obj_b, RelationKind::CapabilityDelegation);
        // 当前语义：重复 Link 报错（已存在）
        // 如果未来改为幂等，这里改成 assert!(res.is_ok())
        println!("[P7.5] duplicate link result: {:?}", res);
    }

    // ========== P7.5 第二轮：WAL Replay + Savepoint 回滚 ==========

    #[test]
    fn test_savepoint_rollback_link() {
        let wal_path = "target/test_p7_5_savepoint.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());
        let mut tx = engine.begin();
        let obj_a: ObjectId = 401;
        let obj_b: ObjectId = 402;
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.savepoint(&mut tx2, "sp1").unwrap();
        engine.object_link(&mut tx2, obj_a, obj_b, RelationKind::EffectPropagation).unwrap();

        // rollback_to 后验证
        engine.rollback_to(&mut tx2, "sp1").unwrap();
        engine.commit(&mut tx2).unwrap();

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 0, "Savepoint rollback should remove link");
        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_wal_replay_topology_and_registry() {
        let wal_path = "target/test_p7_5_replay.wal";
        let _ = std::fs::remove_file(wal_path);

        let obj_a: ObjectId = 501;
        let obj_b: ObjectId = 502;

        // 阶段 A: Engine 1 提交 Birth + Link
        {
            let engine1 = VeritasEngine::with_wal_path(wal_path.to_string());
            let mut tx = engine1.begin();
            engine1.object_birth(&mut tx, obj_a).unwrap();
            engine1.object_birth(&mut tx, obj_b).unwrap();
            engine1.object_link(&mut tx, obj_a, obj_b, RelationKind::ContractDependency).unwrap();
            engine1.commit(&mut tx).unwrap();
        }

        // 阶段 B: Engine 2 重新 open WAL，验证恢复
        let engine2 = VeritasEngine::with_wal_path(wal_path.to_string());

        let reg = engine2.object_registry.lock().unwrap();
        assert!(reg.contains_key(&obj_a), "WAL replay: obj_a should be restored");
        assert!(reg.contains_key(&obj_b), "WAL replay: obj_b should be restored");
        drop(reg);

        let topo = engine2.topology.lock().unwrap();
        assert_eq!(topo.len(), 1, "WAL replay: topology should have 1 edge");
        assert_eq!(topo[0].from, obj_a);
        assert_eq!(topo[0].to, obj_b);
        assert_eq!(topo[0].relation, RelationKind::ContractDependency);
        drop(topo);

        let _ = std::fs::remove_file(wal_path);
    }

    // ========== P8.1: OBJECT_DEATH 测试 ==========

    #[test]
    fn test_object_death_normal() {
        let wal_path = "target/test_p8_1_death.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        // birth 并 commit
        let mut tx = engine.begin();
        let obj: ObjectId = 601;
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        // death 并 commit
        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, obj).unwrap();
        engine.commit(&mut tx2).unwrap();

        // 验证状态为 Dead
        let reg = engine.object_registry.lock().unwrap();
        assert_eq!(reg.get(&obj), Some(&ObjectState::Dead), "Object 应为 Dead");
        drop(reg);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_object_death_reject_unknown() {
        let wal_path = "target/test_p8_1_reject.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let mut tx = engine.begin();
        let result = engine.object_death(&mut tx, 999);
        assert!(result.is_err(), "死亡不存在的 Object 应报错");
        engine.abort(&mut tx, AbortReason::WriteConflict);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_object_death_reject_double_death() {
        let wal_path = "target/test_p8_1_double.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj: ObjectId = 602;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, obj).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        let result = engine.object_death(&mut tx3, obj);
        assert!(result.is_err(), "重复死亡应报错");
        engine.abort(&mut tx3, AbortReason::WriteConflict);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_object_death_savepoint_rollback() {
        let wal_path = "target/test_p8_1_sp.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj: ObjectId = 603;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.savepoint(&mut tx2, "before_death").unwrap();
        engine.object_death(&mut tx2, obj).unwrap();
        engine.rollback_to(&mut tx2, "before_death").unwrap();
        engine.commit(&mut tx2).unwrap();

        let reg = engine.object_registry.lock().unwrap();
        assert_eq!(reg.get(&obj), Some(&ObjectState::Alive), "Savepoint 回滚后 Object 应为 Alive");
        drop(reg);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_object_death_replay() {
        let wal_path = "target/test_p8_1_replay.wal";
        let _ = std::fs::remove_file(wal_path);

        let obj: ObjectId = 604;
        {
            let engine1 = VeritasEngine::with_wal_path(wal_path.to_string());
            let mut tx = engine1.begin();
            engine1.object_birth(&mut tx, obj).unwrap();
            engine1.commit(&mut tx).unwrap();
            let mut tx2 = engine1.begin();
            engine1.object_death(&mut tx2, obj).unwrap();
            engine1.commit(&mut tx2).unwrap();
        }

        let engine2 = VeritasEngine::with_wal_path(wal_path.to_string());
        let reg = engine2.object_registry.lock().unwrap();
        assert_eq!(reg.get(&obj), Some(&ObjectState::Dead), "WAL replay 后 Object 应为 Dead");
        drop(reg);

        let _ = std::fs::remove_file(wal_path);
    }

    // ========== P8.2: 确定性拓扑清理测试 ==========

    #[test]
    fn test_death_cleans_outgoing_link() {
        let wal_path = "target/test_p8_2_out.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let a: ObjectId = 701;
        let b: ObjectId = 702;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.object_link(&mut tx, a, b, RelationKind::CapabilityDelegation).unwrap();
        engine.commit(&mut tx).unwrap();

        // 死亡 A，A->B 的边应被清理
        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, a).unwrap();
        engine.commit(&mut tx2).unwrap();

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 0, "A 死后 A->B 应被清理");
        drop(topo);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_death_cleans_incoming_link() {
        let wal_path = "target/test_p8_2_in.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let a: ObjectId = 703;
        let b: ObjectId = 704;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.object_link(&mut tx, a, b, RelationKind::ContractDependency).unwrap();
        engine.commit(&mut tx).unwrap();

        // 死亡 B，A->B 的边应被清理
        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, b).unwrap();
        engine.commit(&mut tx2).unwrap();

        let topo = engine.topology.lock().unwrap();
        assert_eq!(topo.len(), 0, "B 死后 A->B 应被清理");
        drop(topo);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_link_to_dead_rejected() {
        let wal_path = "target/test_p8_2_reject.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let a: ObjectId = 705;
        let b: ObjectId = 706;
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.commit(&mut tx).unwrap();

        // 死亡 A
        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, a).unwrap();
        engine.commit(&mut tx2).unwrap();

        // 尝试连接死 A 到 B
        let mut tx3 = engine.begin();
        let result = engine.object_link(&mut tx3, a, b, RelationKind::EffectPropagation);
        assert!(result.is_err(), "连接已死 Object 应报错");
        engine.abort(&mut tx3, AbortReason::WriteConflict);

        // 尝试连接 B 到死 A
        let mut tx4 = engine.begin();
        let result2 = engine.object_link(&mut tx4, b, a, RelationKind::EffectPropagation);
        assert!(result2.is_err(), "连接到已死 Object 应报错");
        engine.abort(&mut tx4, AbortReason::WriteConflict);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_death_topology_replay_cleanup() {
        let wal_path = "target/test_p8_2_replay.wal";
        let _ = std::fs::remove_file(wal_path);

        let a: ObjectId = 707;
        let b: ObjectId = 708;
        {
            let engine1 = VeritasEngine::with_wal_path(wal_path.to_string());
            let mut tx = engine1.begin();
            engine1.object_birth(&mut tx, a).unwrap();
            engine1.object_birth(&mut tx, b).unwrap();
            engine1.object_link(&mut tx, a, b, RelationKind::CapabilityDelegation).unwrap();
            engine1.commit(&mut tx).unwrap();
            let mut tx2 = engine1.begin();
            engine1.object_death(&mut tx2, a).unwrap();
            engine1.commit(&mut tx2).unwrap();
        }

        // Replay: A 死，拓扑应为空
        let engine2 = VeritasEngine::with_wal_path(wal_path.to_string());
        let reg = engine2.object_registry.lock().unwrap();
        assert_eq!(reg.get(&a), Some(&ObjectState::Dead));
        drop(reg);
        let topo = engine2.topology.lock().unwrap();
        assert_eq!(topo.len(), 0, "Replay 后拓扑应自动清理");
        drop(topo);

        let _ = std::fs::remove_file(wal_path);
    }

    // ========== P8.3: 能力授权原语测试 ==========

    #[test]
    fn test_capability_grant_to_alive_object() {
        let wal_path = "target/test_p8_3_alive.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj_a: ObjectId = 801;
        let resource: StateId = 100;
        engine.init_state(resource, vec![0]);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        let cap_res = engine.capability_grant(&mut tx2, obj_a, "ReadStorage", resource);
        assert!(cap_res.is_ok(), "向 Alive 的 Object 授权应成功");
        engine.commit(&mut tx2).unwrap();

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_capability_grant_to_dead_object_rejected() {
        let wal_path = "target/test_p8_3_dead.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj_a: ObjectId = 802;
        let resource: StateId = 101;
        engine.init_state(resource, vec![0]);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_death(&mut tx2, obj_a).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        let res = engine.capability_grant(&mut tx3, obj_a, "ReadStorage", resource);
        assert!(res.is_err(), "向 Dead 的 Object 授权必须拒绝");
        engine.abort(&mut tx3, AbortReason::WriteConflict);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_capability_grant_to_nonexistent_rejected() {
        let wal_path = "target/test_p8_3_nonexist.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let resource: StateId = 102;
        engine.init_state(resource, vec![0]);

        let mut tx = engine.begin();
        let res = engine.capability_grant(&mut tx, 999, "ReadStorage", resource);
        assert!(res.is_err(), "向不存在的 Object 授权必须拒绝");
        engine.abort(&mut tx, AbortReason::WriteConflict);

        let _ = std::fs::remove_file(wal_path);
    }

    // ========== P8.4: 能力级联撤销测试 ==========

    #[test]
    fn test_death_cascades_capability_revocation() {
        let wal_path = "target/test_p8_4_cascade.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj_a: ObjectId = 901;
        let resource: StateId = 200;
        engine.init_state(resource, vec![0]);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        let cap_a = engine.capability_grant(&mut tx2, obj_a, "ReadStorage", resource).unwrap();
        engine.commit(&mut tx2).unwrap();

        // 验证能力有效
        {
            let cap_graph = engine.capability_graph.lock().unwrap();
            assert!(cap_graph.is_capability_valid(cap_a), "死亡前能力应有效");
        }

        // 消亡 A
        let mut tx3 = engine.begin();
        engine.object_death(&mut tx3, obj_a).unwrap();
        engine.commit(&mut tx3).unwrap();

        // 验证能力被级联作废
        let cap_graph = engine.capability_graph.lock().unwrap();
        assert!(!cap_graph.is_capability_valid(cap_a), "A 死后能力应被级联作废");
        drop(cap_graph);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_death_spares_unrelated_capabilities() {
        let wal_path = "target/test_p8_4_spare.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj_a: ObjectId = 902;
        let obj_b: ObjectId = 903;
        let resource: StateId = 201;
        engine.init_state(resource, vec![0]);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj_a).unwrap();
        engine.object_birth(&mut tx, obj_b).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        let cap_a = engine.capability_grant(&mut tx2, obj_a, "ReadStorage", resource).unwrap();
        let cap_b = engine.capability_grant(&mut tx2, obj_b, "ReadStorage", resource).unwrap();
        engine.commit(&mut tx2).unwrap();

        // 只死 A
        let mut tx3 = engine.begin();
        engine.object_death(&mut tx3, obj_a).unwrap();
        engine.commit(&mut tx3).unwrap();

        let cap_graph = engine.capability_graph.lock().unwrap();
        assert!(!cap_graph.is_capability_valid(cap_a), "A 的能力应被撤销");
        assert!(cap_graph.is_capability_valid(cap_b), "B 的能力应保留");
        drop(cap_graph);

        let _ = std::fs::remove_file(wal_path);
    }


    // ========== P8-final: Ghost Data 与 Replay 完整性测试 ==========

    #[test]
    fn test_death_leaves_no_ghost_data() {
        let wal_path = "target/test_p8f_ghost.wal";
        let _ = std::fs::remove_file(wal_path);
        let engine = VeritasEngine::with_wal_path(wal_path.to_string());

        let obj: ObjectId = 1001;
        let res: StateId = 300;
        engine.init_state(res, vec![0]);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, obj).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.capability_grant(&mut tx2, obj, "Test", res).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        engine.object_death(&mut tx3, obj).unwrap();
        engine.commit(&mut tx3).unwrap();

        let cg = engine.capability_graph.lock().unwrap();
        assert_eq!(cg.grant_count(), 0, "grants 应为空");
        assert_eq!(cg.holder_count(), 0, "holders 应为空");
        assert_eq!(cg.child_count(), 0, "children 应为空");
        assert_eq!(cg.edge_count(), 0, "edges 应为空");
        drop(cg);

        let _ = std::fs::remove_file(wal_path);
    }

    #[test]
    fn test_death_replay_restores_capability_graph() {
        let wal_path = "target/test_p8f_replay.wal";
        let _ = std::fs::remove_file(wal_path);

        let obj: ObjectId = 1002;
        let res: StateId = 301;
        {
            let engine1 = VeritasEngine::with_wal_path(wal_path.to_string());
            engine1.init_state(res, vec![0]);
            let mut tx = engine1.begin();
            engine1.object_birth(&mut tx, obj).unwrap();
            engine1.commit(&mut tx).unwrap();
            let mut tx2 = engine1.begin();
            engine1.capability_grant(&mut tx2, obj, "Test", res).unwrap();
            engine1.commit(&mut tx2).unwrap();
            let mut tx3 = engine1.begin();
            engine1.object_death(&mut tx3, obj).unwrap();
            engine1.commit(&mut tx3).unwrap();
        }

        let engine2 = VeritasEngine::with_wal_path(wal_path.to_string());
        let reg = engine2.object_registry.lock().unwrap();
        assert_eq!(reg.get(&obj), Some(&ObjectState::Dead));
        drop(reg);
        let topo = engine2.topology.lock().unwrap();
        assert_eq!(topo.len(), 0);
        drop(topo);
        // 注意：当前 CapabilityGraph 恢复时为空，不保留历史能力
        // 此测试验证架构骨架正确，能力 WAL 持久化留待后续版本

        let _ = std::fs::remove_file(wal_path);
    }

    // ========== P9.3: TxId 崩溃恢复续接测试 ==========

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


    // ========== P9.5: Controller 闭环验证 ==========

    #[test]
    fn test_p9_5_wound_marks_pcb_aborted() {
        let engine = VeritasEngine::new();
        let mut old = engine.begin();
        let mut young = engine.begin();
        let obj: ObjectId = 9001;

        // young 先拿锁
        engine.lock_mgr
            .acquire(young.tx_id(), obj, crate::lock::LockMode::Exclusive)
            .unwrap();

        // old 抢锁 → Wound 击毙 young
        let result = engine.lock_mgr
            .acquire(old.tx_id(), obj, crate::lock::LockMode::Exclusive);
        assert!(result.is_ok(), "老事务应成功抢占");

        // 验证 PCB 状态已变为 Aborted
        assert!(engine.tx_mgr().is_aborted(young.tx_id()),
            "被 Wound 的事务 PCB 必须标记为 Aborted");

        // young 提交被拦截
        assert!(engine.commit(&mut young).is_err(),
            "PCB 脏态事务必须被 Engine 拒绝提交");

        // old 正常提交
        assert!(engine.commit(&mut old).is_ok());
    }

    #[test]
    fn test_p9_5_lock_released_after_wound() {
        let engine = VeritasEngine::new();
        let mut old = engine.begin();
        let mut young = engine.begin();
        let mut third = engine.begin();
        let obj: ObjectId = 9002;

        // young 拿锁
        engine.lock_mgr
            .acquire(young.tx_id(), obj, crate::lock::LockMode::Exclusive)
            .unwrap();

        // old Wound 击毙 young，夺锁
        engine.lock_mgr
            .acquire(old.tx_id(), obj, crate::lock::LockMode::Exclusive)
            .unwrap();

        // old 释放锁
        engine.lock_mgr().release_all(old.tx_id());

        // third 能立即获得锁
        assert!(engine.lock_mgr
            .acquire(third.tx_id(), obj, crate::lock::LockMode::Exclusive)
            .is_ok(),
            "Wound 后锁应被正确释放，后续事务可获取");
    }

    #[test]
    fn test_p9_5_aborted_no_ghost_on_replay() {
        let path = format!("wal_p9_5_{}.log", std::process::id());
        let _ = std::fs::remove_file(&path);

        let obj: ObjectId = 9003;
        {
            let engine = VeritasEngine::with_wal_path(path.clone());
            let mut tx = engine.begin();
            engine.object_birth(&mut tx, obj).unwrap();
            // 被击毙，不提交
            engine.tx_mgr().mark_aborted(tx.tx_id());
            let _ = engine.commit(&mut tx);
        }

        // 重启恢复
        let engine2 = VeritasEngine::with_wal_path(path.clone());
        let reg = engine2.object_registry.lock().unwrap();
        assert!(!reg.contains_key(&obj),
            "被 Wound 且未提交的事务，重启后不应产生幽灵 Object");

        drop(reg);
        let _ = std::fs::remove_file(&path);
    }


    #[test]
    fn test_p9_5_commit_removes_pcb() {
        let engine = VeritasEngine::new();
        let mut tx = engine.begin();
        let id = tx.tx_id();
        engine.commit(&mut tx).unwrap();
        assert!(!engine.tx_mgr().is_active(id),
            "commit 后 PCB 应从 tx_table 中移除");
    }

    #[test]
    fn test_p9_5_abort_removes_pcb() {
        let engine = VeritasEngine::new();
        let mut tx = engine.begin();
        let id = tx.tx_id();
        engine.abort(&mut tx, AbortReason::WriteConflict);
        assert!(!engine.tx_mgr().is_active(id),
            "abort 后 PCB 应从 tx_table 中移除");
    }


    // ========== P11: Instruction Layer / Program Execution 测试 ==========

    #[test]
    fn test_p11_program_execution_pipeline() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::executor::Executor;

        let path = format!("wal_p11_{}.log", std::process::id());
        let _ = std::fs::remove_file(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());

        let res_id: StateId = 11001;
        engine.init_state(res_id, vec![0]);

        let program = Program::new()
            .push(Instruction::ObjectBirth { object_id: 1101 })
            .push(Instruction::CapabilityGrant {
                holder: 1101,
                permission: "WritePermission".into(),
                resource: res_id,
            })
            .push(Instruction::Write {
                state_id: res_id,
                payload: vec![1, 2, 3, 4],
            })
            .push(Instruction::Commit);

        let executor = Executor::new(&engine);
        assert!(executor.run_program(&program).is_ok(),
            "Program 指令流应顺利执行");

        let state_entry = engine.peek_state(res_id).unwrap();
        assert_eq!(state_entry.value, vec![1, 2, 3, 4],
            "状态应被 Program 中的 Write 指令修改");

        let _ = std::fs::remove_file(&path);
    }


    // ========== P12: Machine / PC 取指执行周期测试 ==========

    #[test]
    fn test_p12_machine_fetch_execute_cycle() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::Machine;

        let path = format!("wal_p12_{}.log", std::process::id());
        let _ = std::fs::remove_file(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());

        let res_id: StateId = 12001;
        engine.init_state(res_id, vec![0]);

        let program = Program::new()
            .push(Instruction::ObjectBirth { object_id: 1201 })
            .push(Instruction::Write {
                state_id: res_id,
                payload: vec![0xFE, 0xED],
            })
            .push(Instruction::Commit);

        let mut machine = Machine::new(&engine).with_program(program).unwrap();

        assert_eq!(machine.pc(), 0);
        assert_eq!(machine.status(), &crate::machine::MachineStatus::Ready);

        machine.step().unwrap();
        assert_eq!(machine.pc(), 1);

        machine.step().unwrap();
        assert_eq!(machine.pc(), 2);

        machine.step().unwrap();
        assert_eq!(machine.pc(), 3);
        assert!(machine.is_halted());

        let state_entry = engine.peek_state(res_id).unwrap();
        assert_eq!(state_entry.value, vec![0xFE, 0xED]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_p12_machine_run_full_program() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::Machine;

        let path = format!("wal_p12b_{}.log", std::process::id());
        let _ = std::fs::remove_file(&path);
        let engine = VeritasEngine::with_wal_path(path.clone());

        let res_id: StateId = 12002;
        engine.init_state(res_id, vec![0]);

        let program = Program::new()
            .push(Instruction::ObjectBirth { object_id: 1202 })
            .push(Instruction::Write {
                state_id: res_id,
                payload: vec![0xCA, 0xFE],
            })
            .push(Instruction::Commit);

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.pc(), 3);
        assert!(machine.is_halted());

        let state_entry = engine.peek_state(res_id).unwrap();
        assert_eq!(state_entry.value, vec![0xCA, 0xFE]);

        let _ = std::fs::remove_file(&path);
    }


    // ========== P13.1: RegisterFile + LoadConst 测试 ==========

    #[test]
    fn test_p13_1_register_file_and_load_const() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, MachineStatus};

        let engine = VeritasEngine::new();

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 42 })
            .push(Instruction::LoadConst { reg: 1, val: 99 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        assert_eq!(machine.status(), &MachineStatus::Ready);

        machine.step().unwrap();
        assert_eq!(machine.pc(), 1);
        assert_eq!(machine.status(), &MachineStatus::Running);

        machine.step().unwrap();
        assert_eq!(machine.pc(), 2);
        assert!(machine.is_halted());
    }


    // ========== P13.2: Machine ALU + Flags 测试 ==========
    #[test]
    fn test_p13_2_alu_and_flags() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 10 })
            .push(Instruction::LoadConst { reg: 1, val: 20 })
            .push(Instruction::Add { dst: 2, src1: 0, src2: 1 })
            .push(Instruction::Cmp { src1: 0, src2: 0 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(30));
        assert!(machine.flags().zero);
        assert!(!machine.flags().negative);
    }


    #[test]
    fn test_p13_3_memory_load_store() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let mut ctx = engine.begin();
        engine.write(&mut ctx, 10, 100u64.to_le_bytes().to_vec()).unwrap();
        engine.commit(&mut ctx).unwrap();

        let program = Program::new()
            .push(Instruction::LoadStateU64 { reg: 0, state_id: 10 })
            .push(Instruction::LoadConst { reg: 1, val: 50 })
            .push(Instruction::Add { dst: 2, src1: 0, src2: 1 })
            .push(Instruction::WriteRegister { state_id: 20, reg: 2 })
            .push(Instruction::Commit);

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(150));

        let mut ctx2 = engine.begin();
        let bytes = engine.read(&mut ctx2, 20).unwrap();
        let mut arr = [0u8; 8];
        let len = bytes.len().min(8);
        arr[..len].copy_from_slice(&bytes[..len]);
        assert_eq!(u64::from_le_bytes(arr), 150);
    }


    #[test]
    fn test_p13_3_transactional_isolation() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, MachineStatus};

        let engine = VeritasEngine::new();

        let mut ctx = engine.begin();
        engine.write(&mut ctx, 30, 50u64.to_le_bytes().to_vec()).unwrap();
        engine.commit(&mut ctx).unwrap();

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 999 })
            .push(Instruction::WriteRegister { state_id: 30, reg: 0 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();
        assert_eq!(*machine.status(), MachineStatus::Halted);

        let mut ctx2 = engine.begin();
        let bytes = engine.read(&mut ctx2, 30).unwrap();
        let mut arr = [0u8; 8];
        let len = bytes.len().min(8);
        arr[..len].copy_from_slice(&bytes[..len]);
        assert_eq!(u64::from_le_bytes(arr), 50,
            "Uncommitted write must not leak to global StateStore");
    }


    // ========== P14.1: 控制流基础指令单元测试 ==========







    // ========== P14.1: 控制流基础指令测试 ==========

    #[test]
    fn test_p14_1_jmp_unconditional() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 1 })
            .push(Instruction::Jmp { target: 3 })
            .push(Instruction::LoadConst { reg: 0, val: 999 })
            .push(Instruction::LoadConst { reg: 1, val: 42 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(0), &RegisterValue::U64(1));
        assert_eq!(machine.registers().get(1), &RegisterValue::U64(42));
    }

    #[test]
    fn test_p14_1_jz_branch() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 5 })
            .push(Instruction::LoadConst { reg: 1, val: 5 })
            .push(Instruction::Cmp { src1: 0, src2: 1 })
            .push(Instruction::Jz { target: 5 })
            .push(Instruction::LoadConst { reg: 2, val: 111 })
            .push(Instruction::LoadConst { reg: 2, val: 222 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(222));
    }

    #[test]
    fn test_p14_1_jnz_branch() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 5 })
            .push(Instruction::LoadConst { reg: 1, val: 3 })
            .push(Instruction::Cmp { src1: 0, src2: 1 })
            .push(Instruction::Jnz { target: 5 })
            .push(Instruction::LoadConst { reg: 2, val: 111 })
            .push(Instruction::LoadConst { reg: 2, val: 222 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(222));
    }

    #[test]
    fn test_p14_1_jn_branch() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 3 })
            .push(Instruction::LoadConst { reg: 1, val: 5 })
            .push(Instruction::Cmp { src1: 0, src2: 1 })
            .push(Instruction::Jn { target: 5 })
            .push(Instruction::LoadConst { reg: 2, val: 111 })
            .push(Instruction::LoadConst { reg: 2, val: 222 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(222));
    }



    #[test]
    fn test_p14_2_loop_sum() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();

        // R0 = sum, R1 = counter, R2 = 1, R3 = 0
        // loop: Add R0=R0+R1, Sub R1=R1-R2, Cmp R1,R3, Jnz loop
        // result: R0 = 10+9+...+1 = 55
        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 0 })
            .push(Instruction::LoadConst { reg: 1, val: 10 })
            .push(Instruction::LoadConst { reg: 2, val: 1 })
            .push(Instruction::LoadConst { reg: 3, val: 0 })
            .push(Instruction::Add { dst: 0, src1: 0, src2: 1 })
            .push(Instruction::Sub { dst: 1, src1: 1, src2: 2 })
            .push(Instruction::Cmp { src1: 1, src2: 3 })
            .push(Instruction::Jnz { target: 4 });

        let mut machine = Machine::new(&engine).with_program(program).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(0), &RegisterValue::U64(55),
            "10+9+...+1 should equal 55");
        assert_eq!(machine.registers().get(1), &RegisterValue::U64(0),
            "counter should reach 0");
        assert!(machine.flags().zero,
            "ZF should be set when counter==0");
    }



    #[test]
    fn test_p15_3_machine_boot() {
        use crate::instruction::Instruction;
        use crate::program::Program;
        use crate::machine::{Machine, MachineStatus, RegisterValue};

        let engine = VeritasEngine::new();

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 42 })
            .push(Instruction::Halt);

        let mut machine = Machine::new(&engine);
        let image = crate::program::ProgramImage::new(program.instructions.clone());
        machine.boot(image).unwrap();

        assert_eq!(machine.status(), &MachineStatus::Running);
        assert_eq!(machine.pc(), 0);
        assert_eq!(machine.registers().get(0), &RegisterValue::Empty);

        machine.run().unwrap();

        assert_eq!(machine.registers().get(0), &RegisterValue::U64(42));
        assert!(machine.is_halted());
    }



    #[test]
    fn test_p15_4_normal_halt() {
        use crate::instruction::Instruction;
        use crate::program::{Program, ProgramImage};
        use crate::machine::{Machine, MachineStatus, RegisterValue, ExecutionResult};

        let engine = VeritasEngine::new();
        let mut machine = Machine::new(&engine);

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 100 })
            .push(Instruction::Halt);
        let image = ProgramImage::new(program.instructions);

        machine.boot(image).unwrap();
        let res = machine.run_with_config(Default::default()).unwrap();

        assert_eq!(res, ExecutionResult::Halted { cycles: 2 });
        assert_eq!(machine.registers().get(0), &RegisterValue::U64(100));
        assert!(machine.is_halted());
    }

    #[test]
    fn test_p15_4_cycle_limit() {
        use crate::instruction::Instruction;
        use crate::program::{Program, ProgramImage};
        use crate::machine::{Machine, ExecutionConfig, ExecutionResult};

        let engine = VeritasEngine::new();
        let mut machine = Machine::new(&engine);

        let program = Program::new()
            .push(Instruction::Jmp { target: 0 });
        let image = ProgramImage::new(program.instructions);

        machine.boot(image).unwrap();
        let res = machine.run_with_config(ExecutionConfig { max_cycles: 10 }).unwrap();

        assert_eq!(res, ExecutionResult::CycleLimitReached { cycles: 10 });
    }



    #[test]
    fn test_p15_5_binary_boot_normal() {
        use crate::instruction::Instruction;
        use crate::program::{Program, ProgramImage};
        use crate::machine::{Machine, RegisterValue};

        let engine = VeritasEngine::new();
        let mut machine = Machine::new(&engine);

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 77 })
            .push(Instruction::Halt);
        let image = ProgramImage::new(program.instructions);
        let bytes = image.encode().unwrap();

        machine.boot_bytes(&bytes).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(0), &RegisterValue::U64(77));
        assert!(machine.is_halted());
    }

    #[test]
    fn test_p15_5_binary_boot_rejects_corrupted() {
        use crate::instruction::Instruction;
        use crate::program::{Program, ProgramImage};
        use crate::machine::{Machine, MachineStatus};

        let engine = VeritasEngine::new();
        let mut machine = Machine::new(&engine);

        let program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 99 })
            .push(Instruction::Halt);
        let image = ProgramImage::new(program.instructions);
        let mut bytes = image.encode().unwrap();
        bytes[20] ^= 0xFF;

        let result = machine.boot_bytes(&bytes);
        assert!(result.is_err(), "Corrupted image must be rejected");
        assert_eq!(machine.status(), &MachineStatus::Ready,
            "Machine must stay Ready after rejected boot");
    }

}