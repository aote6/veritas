use crate::checkpoint::Checkpoint;
use crate::history::{ExecutionHistory, ReplayRecord};
use crate::state_memory::StateMemory;
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

#[allow(dead_code)]
fn bytes_to_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}


pub struct VeritasEngine {
    global_version: AtomicU64,
    state_store: StateStore,
    scope_registry: ScopeRegistry,
    commit_lock: Mutex<()>,
    wal: WalWriter,
    object_registry: Mutex<HashMap<ObjectId, crate::types::ObjectRecord>>,
    topology: Mutex<Vec<LinkEdge>>,
    capability_graph: Mutex<CapabilityGraph>,
    #[allow(dead_code)]
    tx_mgr: Arc<TransactionManager>,
    #[allow(dead_code)]
    lock_mgr: Arc<LockManager>,
    controller: TransactionController,
    pub state_memory: std::sync::Mutex<StateMemory>,
    pub history: std::sync::Mutex<ExecutionHistory>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOperation {
    Write,
    Freeze,
    Death,
}

impl crate::types::ObjectState {
    #[inline]
    pub fn allows(self, op: ObjectOperation) -> bool {
        match (self, op) {
            (crate::types::ObjectState::Alive, _) => true,
            (crate::types::ObjectState::Frozen, ObjectOperation::Death) => true,
            (crate::types::ObjectState::Frozen, _) => false,
            (crate::types::ObjectState::Dead, _) => false,
        }
    }
}

impl VeritasEngine {
    /// 宪法级 API：查询指定 Object 的当前生命周期状态
    pub fn get_object_state(&self, object_id: crate::types::ObjectId) -> Option<crate::types::ObjectState> {
        let registry = self.object_registry.lock().unwrap();
        registry.get(&object_id).map(|r| r.state)
    }

    /// 宪法级 API：判断 Object 是否已进入 Dead 状态
    pub fn is_object_dead(&self, object_id: crate::types::ObjectId) -> bool {
        self.get_object_state(object_id) == Some(crate::types::ObjectState::Dead)
    }

    pub fn attach_capability(&self, ctx: &mut crate::types::TransactionContext, cap_id: u64) {
        ctx.capabilities.push(cap_id);
    }

    fn verify_capability(&self, ctx: &crate::types::TransactionContext) -> Result<(), crate::types::VeritasError> {
        if !ctx.capability_enforced {
            return Ok(());
        }

        let cap_graph = self.capability_graph.lock().unwrap();

        for (addr, _) in &ctx.write_set.changes {
            let has_valid_cap = ctx.capabilities.iter().any(|cap_id| {
                cap_graph.is_capability_valid(*cap_id)
                    && cap_graph
                        .info(*cap_id)
                        .map(|info| info.resource == addr.object_id)
                        .unwrap_or(false)
            });

            if !has_valid_cap {
                return Err(crate::types::VeritasError::PermissionDenied);
            }
        }

        Ok(())
    }

    pub fn record_history(&self, ctx: &crate::types::TransactionContext) {
        let before = self.state_root();
        let write_set = &ctx.write_set;
        self.apply_state_memory(ctx, write_set);
        let after = self.state_root();
        if let Ok(mut hist) = self.history.lock() {
            let writes_for_record: Vec<(crate::types::Address, Vec<u8>)> = write_set.changes.iter().map(|(addr, val)| (*addr, val.clone())).collect();
        hist.push(ReplayRecord::new(ctx.tx_id(), ctx.capabilities.clone(), ctx.program_hash.unwrap_or(0), writes_for_record, before, after));
        }
    }

    pub fn create_checkpoint(&self) -> Checkpoint {
        Checkpoint::new(self.state_memory.lock().unwrap().snapshot())
    }

    pub fn restore_checkpoint(&self, ck: &Checkpoint) -> bool {
        if !ck.verify() { return false; }
        self.state_memory.lock().unwrap().restore(&ck.snapshot);
        true
    }

    pub fn apply_state_memory(&self, _ctx: &crate::types::TransactionContext, write_set: &crate::types::WriteSet) {
        if let Ok(mut mem) = self.state_memory.lock() {
            for (addr, payload) in &write_set.changes {
                mem.write(*addr, payload.clone());
            }
        }
    }

    pub fn state_root(&self) -> u64 {
        self.state_memory.lock().unwrap().root_hash()
    }

    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = format!("wal_{}_{:?}_{}.log", std::process::id(), std::thread::current().id(), n);
        let _ = std::fs::remove_file(&path);
        Self::with_wal_path(path)
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
        let mut recovered_objects: HashMap<ObjectId, crate::types::ObjectRecord> = HashMap::new();
        let mut recovered_links: Vec<LinkEdge> = Vec::new();
        let mut recovered_deaths: Vec<ObjectId> = Vec::new();
        for record in &records {
            match record {
                WalEntry::ObjectBirth { object_id, .. } => {
                    recovered_objects.insert(*object_id, crate::types::ObjectRecord::new_state(*object_id));
                }
                WalEntry::ObjectDeath { object_id, .. } => {
                    { let mut r = crate::types::ObjectRecord::new_state(*object_id); r.state = crate::types::ObjectState::Dead; recovered_objects.insert(*object_id, r); }
                    recovered_links.retain(|edge| edge.from != *object_id && edge.to != *object_id);
                    recovered_deaths.push(*object_id);
                }
                WalEntry::ObjectLink { from, to, link_type, .. } => {
                    let relation = match link_type {
                        0 => LinkType::DependsOn,
                        1 => LinkType::Owns,
                        2 => LinkType::References,
                        _ => continue,
                    };
                    recovered_links.push(LinkEdge { from: *from, to: *to, link_type: relation });
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
            // 临时占位：WAL目前不记录object_id，恢复的状态统一归属内核Object(0)。
            // 待WAL格式扩展支持Object寻址后应移除此转换。
            state_store: StateStore::from_map(
                state_map.into_iter()
                    .map(|(sid, entry)| (sid, entry))
                    .collect()
            ),
            scope_registry: ScopeRegistry::from_map(scope_map),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open(&wal_path).expect("Failed to open WAL file"),
            object_registry: Mutex::new(recovered_objects),
            topology: Mutex::new(recovered_links),
            capability_graph: Mutex::new(CapabilityGraph::new()),
            tx_mgr,
            lock_mgr,
            controller,
            state_memory: std::sync::Mutex::new(StateMemory::new()),
            history: std::sync::Mutex::new(ExecutionHistory::new()),
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

    /// 注：当前所有非事务上下文的直接状态操作，临时统一归属内核Object(0)。
    /// 待Object生命周期（OBJECT_BIRTH流程）接管后，此处应改为要求调用方
    /// 显式提供object_id。这是过渡期的合理默认，非最终语义。
    pub fn init_state(&self, state_id: StateId, initial_value: Vec<u8>) {
        self.state_store.insert(
            crate::types::Address::new(0, state_id),
            StateEntry {
                value: initial_value,
                version: 0,
            },
        );
    }

    pub fn peek_state(&self, state_id: StateId) -> Option<StateEntry> {
        self.state_store.read(crate::types::Address::new(0, state_id))
    }

    pub fn init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) {
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        ctx.write_set.push(addr, value);
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

    fn grant_base_access(&self, ctx: &mut TransactionContext, object_id: ObjectId) {
        let mut cap_graph = self.capability_graph.lock().unwrap();
        let cap_id = cap_graph.grant(
            "BaseAccess".into(),
            object_id,
            object_id,
            object_id,
        );
        ctx.capabilities.push(cap_id);
    }


    pub fn begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
        let mut ctx = self.begin();
        ctx.enter_object(object_id);
        self.grant_base_access(&mut ctx, object_id);
        ctx
    }
    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        let _addr = crate::types::Address::new(ctx.current_object, state_id);
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        if let Some(written_value) = ctx.write_set.get_latest(addr) {
            return Ok(written_value.clone());
        }

        let entry = self
            .state_store
            .read(crate::types::Address::new(ctx.current_object, state_id))
            .ok_or(VeritasError::EngineError(format!(
                "State {:?} not found",
                state_id
            )))?;

        if entry.version > ctx.snapshot_version() {
            return Err(VeritasError::Abort(AbortReason::ReadFutureVersion));
        }

        ctx.read_set.states.insert(addr, entry.version);
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

        // Object Protection: 目标写入对象必须已在 object_registry 中存在且未被冻结/死亡
        {
            let reg = self.object_registry.lock().unwrap();
            match reg.get(&ctx.current_object) {
                None => {
                    return Err(VeritasError::PermissionDenied);
                }
                Some(r) if r.is_frozen() || r.is_dead() => {
                    return Err(VeritasError::PermissionDenied);
                }
                Some(_) => {}
            }
        }

        let addr = crate::types::Address::new(ctx.current_object, state_id);

        if !ctx.read_set.states.contains_key(&addr) {
            if let Some(entry) = self.state_store.read(crate::types::Address::new(ctx.current_object, state_id)) {
                ctx.read_set.states.insert(addr, entry.version);
            }
        }

        ctx.write_set.push(addr, value);
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

    /// P8.1: OWNS 死亡闭包——沿 OWNS 边传播死亡
    fn expand_owns_death_closure(&self, ctx: &mut TransactionContext) {
        if ctx.pending_deaths.is_empty() {
            return;
        }

        let topo = self.topology.lock().unwrap();
        let unlinked: std::collections::HashSet<(crate::types::ObjectId, crate::types::ObjectId)> =
            ctx.pending_unlinks.iter().copied().collect();

        let mut queue = ctx.pending_deaths.clone();
        let mut seen: std::collections::HashSet<crate::types::ObjectId> = queue.iter().copied().collect();
        let mut i = 0;

        while i < queue.len() {
            let id = queue[i];

            for edge in topo.iter() {
                if edge.from == id
                    && edge.link_type == crate::types::LinkType::Owns
                    && !unlinked.contains(&(edge.from, edge.to))
                    && seen.insert(edge.to)
                {
                    queue.push(edge.to);
                }
            }

            for edge in &ctx.pending_links {
                if edge.from == id
                    && edge.link_type == crate::types::LinkType::Owns
                    && !unlinked.contains(&(edge.from, edge.to))
                    && seen.insert(edge.to)
                {
                    queue.push(edge.to);
                }
            }

            i += 1;
        }

        ctx.pending_deaths = queue;
    }

    pub fn commit(&self, ctx: &mut TransactionContext) -> Result<(), VeritasError> {
        self.controller.pre_commit_check(ctx)?;

        let _lock = self.commit_lock.lock().unwrap();

        self.detect_conflict(ctx)?;
        self.detect_scope_conflict(ctx)?;
        self.verify_capability(ctx)?;

        let commit_version = self.global_version.load(Ordering::Acquire) + 1;

        let mut writes_map = HashMap::new();
        for (addr, value) in ctx.write_set.iter() {
            writes_map.insert(*addr, value.clone());
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

        for (addr, value) in ctx.write_set.iter() {
            self.state_store.insert(
                *addr,
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
                registry.insert(*object_id, crate::types::ObjectRecord::new_state(*object_id));
            }
        }

        // P8.1: OWNS 死亡闭包——from 死亡则 owned 对象一并进入 pending_deaths
        self.expand_owns_death_closure(ctx);

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
                if let Some(r) = registry.get_mut(object_id) { r.state = crate::types::ObjectState::Dead; } else { let mut r = crate::types::ObjectRecord::new_state(*object_id); r.state = crate::types::ObjectState::Dead; registry.insert(*object_id, r); }
            }
            for object_id in ctx.pending_freezes.drain(..) {
                if let Some(r) = registry.get_mut(&object_id) { r.state = crate::types::ObjectState::Frozen; } else { let mut r = crate::types::ObjectRecord::new_state(object_id); r.state = crate::types::ObjectState::Frozen; registry.insert(object_id, r); }
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
                link_type: match edge.link_type {
                    LinkType::DependsOn => 0,
                    LinkType::Owns => 1,
                    LinkType::References => 2,
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
            // P26: 处理 pending_unlinks
            for (from, to) in &ctx.pending_unlinks {
                topo.retain(|e| e.from != *from || e.to != *to);
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
        self.record_history(ctx);

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

    /// P26: OBJECT_FREEZE - 冻结Object，使其变为只读
    pub fn object_freeze(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let registry = self.object_registry.lock().unwrap();
        let record = registry.get(&object_id);
        if record.is_none() || !record.unwrap().state.allows(ObjectOperation::Freeze) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        ctx.pending_freezes.push(object_id);
        Ok(())
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
        let record = registry.get(&object_id).cloned();
        drop(registry);

        match record {
            None => return Err(VeritasError::Abort(AbortReason::WriteConflict)),
            Some(r) if r.is_dead() => return Err(VeritasError::Abort(AbortReason::WriteConflict)),
            Some(_) => {}
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
        relation: LinkType,
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
        let from_dead = reg.get(&from).map(|r| r.is_dead()).unwrap_or(false) || ctx.pending_deaths.contains(&from);
        let to_dead = reg.get(&to).map(|r| r.is_dead()).unwrap_or(false) || ctx.pending_deaths.contains(&to);
        drop(reg);
        if from_dead || to_dead {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        ctx.pending_links.push(LinkEdge { from, to, link_type: relation });
        Ok(())
    }

    /// P26: OBJECT_UNLINK - 移除Object间的Link
    pub fn object_unlink(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        // 标记Link待删除，Commit时生效
        ctx.pending_unlinks.push((from, to));
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

        // birth 时创建者自动获得该 Object 的 AdminCap
        let cap_id = {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.grant(
                "AdminCap".into(),
                object_id,
                object_id,
                object_id,
            )
        };
        ctx.capabilities.push(cap_id);

        Ok(())
    }

    pub fn abort(&self, ctx: &mut TransactionContext, reason: AbortReason) {
        self.controller.abort(ctx, reason);
    }

    fn detect_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        for (addr, read_version) in &ctx.read_set.states {
            if let Some(entry) = self.state_store.read(*addr) {
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

