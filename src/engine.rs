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
use crate::wal::{RecoveryManager, WalEntry, WalWriter};
use crate::store::StateStore;

#[allow(dead_code)]
fn bytes_to_u64(bytes: &[u8]) -> u64 {
    let mut arr = [0u8; 8];
    let len = bytes.len().min(8);
    arr[..len].copy_from_slice(&bytes[..len]);
    u64::from_le_bytes(arr)
}


pub struct VeritasEngine {
    global_version: AtomicU64,
    state_store: StateStore,
    scope_registry: ScopeRegistry,
    commit_lock: Mutex<()>,
    wal: WalWriter,
    wal_path: String,
    object_registry: Mutex<HashMap<ObjectId, crate::types::ObjectRecord>>,
    topology: Mutex<Vec<LinkEdge>>,
    capability_graph: Mutex<CapabilityGraph>,
    #[allow(dead_code)]
    tx_mgr: Arc<TransactionManager>,
    #[allow(dead_code)]
    lock_mgr: Arc<LockManager>,
    controller: TransactionController,

    object_id_counter: AtomicU64,
    /// Test probe: DependencyInvalidated pairs from last commit. Not for production use.
    last_dep_inv: std::sync::Mutex<Vec<(crate::types::ObjectId, crate::types::ObjectId)>>,
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
    /// Test probe: last commit's DependencyInvalidated pairs.
    /// Production consumers must use Effect/WAL path, not this API.
    pub fn last_dependency_invalidations(&self) -> Vec<(crate::types::ObjectId, crate::types::ObjectId)> {
        self.last_dep_inv.lock().unwrap().clone()
    }

    /// 宪法级 API：查询指定 Object 的当前生命周期状态
    pub fn get_object_state(&self, object_id: crate::types::ObjectId) -> Option<crate::types::ObjectState> {
        let registry = self.object_registry.lock().unwrap();
        registry.get(&object_id).map(|r| r.state)
    }

    /// 宪法级 API：判断 Object 是否已进入 Dead 状态
    pub fn is_object_dead(&self, object_id: crate::types::ObjectId) -> bool {
        self.get_object_state(object_id) == Some(crate::types::ObjectState::Dead)
    }

    /// Return all non-Dead ObjectIds known to this engine.
    /// Used by recovery equivalence tests to compare engine states.
    pub fn list_object_ids(&self) -> Vec<crate::types::ObjectId> {
        let registry = self.object_registry.lock().unwrap();
        registry.iter()
            .filter(|(_, r)| !r.is_dead())
            .map(|(id, _)| *id)
            .collect()
    }

    /// PR2.1: 导出 Object 稳定语义快照。不暴露 ObjectRecord。
    pub fn snapshot_objects(&self) -> Vec<crate::types::ObjectSnapshot> {
        let registry = self.object_registry.lock().unwrap();
        let mut result: Vec<crate::types::ObjectSnapshot> = registry
            .iter()
            .map(|(id, record)| {
                crate::types::ObjectSnapshot {
                    id: *id,
                    object_type: record.object_type,
                    lifecycle_state: record.state,
                    metadata: vec![],
                    payload: Self::serialize_object_body(&record.body),
                }
            })
            .collect();
        result.sort_by_key(|o| o.id);
        result
    }

    /// 将 ObjectBody 序列化为稳定字节。不引入外部 serde 依赖。
    pub fn serialize_object_body(body: &crate::types::ObjectBody) -> Vec<u8> {
        match body {
            crate::types::ObjectBody::State => vec![0x00],
            crate::types::ObjectBody::Module {
                code_section,
                import_section,
                export_section,
                verification_rule,
            } => {
                let mut buf = vec![0x01];
                // code_section length + bytes
                buf.extend_from_slice(&(code_section.len() as u32).to_le_bytes());
                buf.extend_from_slice(code_section);
                // import_section length + ids
                buf.extend_from_slice(&(import_section.len() as u32).to_le_bytes());
                for id in import_section {
                    buf.extend_from_slice(&id.to_le_bytes());
                }
                // export_section length + entries
                buf.extend_from_slice(&(export_section.len() as u32).to_le_bytes());
                for (name, idx) in export_section {
                    buf.extend_from_slice(&(name.len() as u32).to_le_bytes());
                    buf.extend_from_slice(name.as_bytes());
                    buf.extend_from_slice(&(*idx as u32).to_le_bytes());
                }
                // verification_rule
                match verification_rule {
                    None => buf.push(0x00),
                    Some(rule) => {
                        buf.push(0x01);
                        buf.extend_from_slice(&rule.max_instances.unwrap_or(0).to_le_bytes());
                        buf.extend_from_slice(&(rule.allow_instructions.len() as u32).to_le_bytes());
                        buf.extend_from_slice(&rule.allow_instructions);
                    }
                }
                buf
            }
        }
    }

    pub fn attach_capability(&self, ctx: &mut crate::types::TransactionContext, cap_id: u64) {
        ctx.capabilities.push(cap_id);
    }

    /// 从 ObjectSnapshot 恢复 ObjectRegistry，清空后重建。
    pub fn restore_objects(&self, snapshots: &[crate::types::ObjectSnapshot]) {
        let mut registry = self.object_registry.lock().unwrap();
        registry.clear();
        for snap in snapshots {
            registry.insert(snap.id, crate::types::ObjectRecord {
                id: snap.id,
                object_type: snap.object_type,
                state: snap.lifecycle_state,
                body: Self::deserialize_object_body(&snap.payload),
            });
        }
    }

    /// 将稳定字节反序列化为 ObjectBody。与 serialize_object_body 互逆。
    pub fn deserialize_object_body(payload: &[u8]) -> crate::types::ObjectBody {
        if payload.is_empty() {
            return crate::types::ObjectBody::State;
        }
        match payload[0] {
            0x00 => crate::types::ObjectBody::State,
            0x01 => {
                let mut pos = 1;
                // code_section
                let len = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                pos += 4;
                let code_section = payload[pos..pos+len].to_vec();
                pos += len;
                // import_section
                let len = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                pos += 4;
                let mut import_section = Vec::new();
                for _ in 0..len {
                    let id = u64::from_le_bytes([
                        payload[pos], payload[pos+1], payload[pos+2], payload[pos+3],
                        payload[pos+4], payload[pos+5], payload[pos+6], payload[pos+7],
                    ]);
                    import_section.push(id);
                    pos += 8;
                }
                // export_section
                let len = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                pos += 4;
                let mut export_section = std::collections::HashMap::new();
                for _ in 0..len {
                    let name_len = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                    pos += 4;
                    let name = String::from_utf8(payload[pos..pos+name_len].to_vec()).unwrap_or_default();
                    pos += name_len;
                    let idx = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                    pos += 4;
                    export_section.insert(name, idx);
                }
                // verification_rule
                let verification_rule = match payload[pos] {
                    0x00 => None,
                    0x01 => {
                        pos += 1;
                        let max_instances = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]);
                        pos += 4;
                        let len = u32::from_le_bytes([payload[pos], payload[pos+1], payload[pos+2], payload[pos+3]]) as usize;
                        pos += 4;
                        let allow_instructions = payload[pos..pos+len].to_vec();
                        Some(crate::types::VerificationRule {
                            max_instances: if max_instances == 0 { None } else { Some(max_instances) },
                            allow_instructions,
                        })
                    }
                    _ => None,
                };
                crate::types::ObjectBody::Module {
                    code_section,
                    import_section,
                    export_section,
                    verification_rule,
                }
            }
            _ => crate::types::ObjectBody::State,
        }
    }

    /// PR2.1: 导出 Link 稳定语义快照。不暴露 LinkEdge。
    pub fn snapshot_links(&self) -> Vec<crate::types::LinkSnapshot> {
        let topo = self.topology.lock().unwrap();
        let mut result: Vec<crate::types::LinkSnapshot> = topo
            .iter()
            .map(|edge| crate::types::LinkSnapshot {
                from: edge.from,
                to: edge.to,
                link_type: edge.link_type,
            })
            .collect();
        result.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)).then((a.link_type as u8).cmp(&(b.link_type as u8))));
        result
    }

    /// 从 LinkSnapshot 恢复 Topology，清空后重建。
    pub fn restore_links(&self, snapshots: &[crate::types::LinkSnapshot]) {
        let mut topo = self.topology.lock().unwrap();
        topo.clear();
        for snap in snapshots {
            topo.push(crate::types::LinkEdge {
                from: snap.from,
                to: snap.to,
                link_type: snap.link_type,
            });
        }
    }

    /// 只读查询：某个 holder 是否持有某个 cap_id 且该 cap 有效。测试与外部诊断用。
    pub fn holds_capability(&self, cap_id: crate::types::CapabilityId, holder: crate::types::ObjectId) -> bool {
        let cap_graph = self.capability_graph.lock().unwrap();
        cap_graph.is_capability_valid(cap_id) && cap_graph.holds(cap_id, holder)
    }

    /// 只读查询：capability_graph 当前的 grant_sequence 计数器值。测试用于推算 cap_id。
    pub fn capability_sequence(&self) -> u64 {
        let cap_graph = self.capability_graph.lock().unwrap();
        cap_graph.current_sequence()
    }

    /// 只读查询：topology 中是否存在 from->to 这条边(任意 link_type)。测试用。
    pub fn has_link(&self, from: ObjectId, to: ObjectId) -> bool {
        let topo = self.topology.lock().unwrap();
        topo.iter().any(|edge| edge.from == from && edge.to == to)
    }

    fn collect_access_intents(ctx: &crate::types::TransactionContext) -> Vec<crate::types::AccessIntent> {
        let mut intents = Vec::new();
        for addr in ctx.read_set.states.keys() {
            intents.push(crate::types::AccessIntent::Read(addr.object_id));
        }
        for (addr, _) in &ctx.write_set.changes {
            intents.push(crate::types::AccessIntent::Write(addr.object_id));
        }
        for edge in &ctx.pending_links {
            intents.push(crate::types::AccessIntent::Link(edge.from, edge.to));
        }
        for (from, to) in &ctx.pending_unlinks {
            intents.push(crate::types::AccessIntent::Unlink(*from, *to));
        }
        for oid in &ctx.pending_deaths {
            intents.push(crate::types::AccessIntent::Destroy(*oid));
        }
        for oid in &ctx.pending_freezes {
            intents.push(crate::types::AccessIntent::Freeze(*oid));
        }
        for oid in &ctx.pending_calls {
            intents.push(crate::types::AccessIntent::Call(*oid));
        }
        intents
    }

    /// Single-intent authorization used by Machine CALL and by commit-time verify.
    /// Self-access (current_object / capability_context) is exempt.
    pub fn authorize_intent(
        &self,
        ctx: &crate::types::TransactionContext,
        intent: &crate::types::AccessIntent,
    ) -> Result<(), crate::types::VeritasError> {
        let cap_graph = self.capability_graph.lock().unwrap();
        for target in intent.target_objects() {
            if target == ctx.current_object || target == ctx.capability_context {
                continue;
            }
            // Attached caps must still be actively held (respects revoke).
            let has_committed = ctx.capabilities.iter().any(|cap_id| {
                let resource_ok = cap_graph
                    .info(*cap_id)
                    .map(|info| info.resource == target)
                    .unwrap_or(false);
                if !resource_ok {
                    return false;
                }
                cap_graph.holds(*cap_id, ctx.current_object)
                    || cap_graph.holds(*cap_id, ctx.capability_context)
            });
            let has_pending = ctx.pending_capabilities.iter().any(|g| {
                g.resource == target
                    && (g.grantee == ctx.current_object
                        || g.grantee == ctx.capability_context
                        || ctx.capabilities.contains(&g.capability_id))
            });
            // Live graph: holder actively holds any capability on target resource.
            // Enables CALL after GRANT/Delegate without requiring attach_capability.
            let has_graph = [ctx.current_object, ctx.capability_context]
                .into_iter()
                .any(|holder| {
                    cap_graph.caps_for_holder(holder).iter().any(|cid| {
                        cap_graph.holds(*cid, holder)
                            && cap_graph
                                .info(*cid)
                                .map(|info| info.resource == target)
                                .unwrap_or(false)
                    })
                });
            if !has_committed && !has_pending && !has_graph {
                return Err(crate::types::VeritasError::PermissionDenied);
            }
        }
        Ok(())
    }

    fn verify_capability(&self, ctx: &crate::types::TransactionContext) -> Result<(), crate::types::VeritasError> {
        // 结构性豁免 + AccessIntent 全覆盖（Read/Write/Link/Unlink/Destroy/Freeze/Call）
        for intent in &Self::collect_access_intents(ctx) {
            self.authorize_intent(ctx, intent)?;
        }
        Ok(())
    }

    /// PR3: 从五组件聚合 WorldSnapshot。Engine 只做协调，不操作子模块内部。
    pub fn create_checkpoint(&self) -> WorldSnapshot {
        let tx_id = self.tx_mgr.current_tx_id();
        let state_entries = self.state_store.snapshot();
        let objects = self.snapshot_objects();
        let links = self.snapshot_links();
        let capability_records = self.capability_graph.lock().unwrap().snapshot_capabilities();
        let scopes = self.scope_registry.snapshot_all_scopes();

        // commitment_hash 直接用 engine 已有的 root_hash，不另写算法
        let commitment_hash = {
            let h = self.state_root();
            let mut bytes = [0u8; 32];
            bytes[0..8].copy_from_slice(&h.to_le_bytes());
            bytes
        };

        let global_version = self.global_version.load(Ordering::Acquire);
        let object_id_counter = self.object_id_counter.load(Ordering::Acquire);
        let grant_sequence = self.capability_graph.lock().unwrap().current_sequence();

        WorldSnapshot {
            commitment_hash,
            tx_id,
            state_entries,
            capability_records,
            objects,
            links,
            scopes,
            global_version,
            object_id_counter,
            grant_sequence,
        }
    }

    /// PR3: 从 WorldSnapshot 恢复五组件 + 续行元数据。
    pub fn restore_checkpoint(&self, snap: &WorldSnapshot) -> bool {
        self.global_version.store(snap.global_version, Ordering::SeqCst);
        self.object_id_counter.store(snap.object_id_counter, Ordering::SeqCst);
        {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.set_grant_sequence(snap.grant_sequence);
            cap_graph.restore_capabilities(&snap.capability_records);
        }
        self.state_store.restore_snapshot(&snap.state_entries);
        self.restore_objects(&snap.objects);
        self.restore_links(&snap.links);
        self.scope_registry.restore_scopes(&snap.scopes);
        true
    }

    pub fn state_root(&self) -> u64 {
        self.root_hash()
    }

    // ========== Stage 3.1: RootHash ==========

    /// FNV-1a 确定性哈希。
    fn deterministic_hash(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &byte in bytes {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 多个 u64 的 LE 8字节拼接后 FNV 哈希。
    fn hash_u64s(items: &[u64]) -> u64 {
        let mut buf = Vec::with_capacity(items.len() * 8);
        for v in items {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Self::deterministic_hash(&buf)
    }

    /// 对集合中每个元素调用 encode 函数拼接字节，最后 FNV 哈希。
    /// 调用方负责在传入前排序。
    fn hash_each<T, F>(items: &[T], encode: F) -> u64
    where
        F: Fn(&T, &mut Vec<u8>),
    {
        let mut buf = Vec::new();
        for item in items {
            encode(item, &mut buf);
        }
        Self::deterministic_hash(&buf)
    }

    /// 计算 WorldState 五组件的确定性根哈希。
    ///
    /// 五组件：StateStore, ObjectRegistry, Topology,
    ///         CapabilityGraph, ScopeRegistry。
    /// 每组件各自排序后独立哈希，最终 H(h1, h2, h3, h4, h5)。
    pub fn root_hash(&self) -> u64 {
        // 1. StateStore — Address 升序
        let mut entries = self.state_store.all_entries();
        entries.sort_by_key(|(addr, _)| *addr);
        let h1 = Self::hash_each(&entries, |(addr, entry), buf| {
            buf.extend_from_slice(&addr.object_id.to_le_bytes());
            buf.extend_from_slice(&addr.state_id.to_le_bytes());
            buf.extend_from_slice(&entry.value);
            buf.extend_from_slice(&entry.version.to_le_bytes());
        });

        // 2. ObjectRegistry — ObjectId 升序
        let mut records: Vec<(ObjectId, crate::types::ObjectRecord)> = {
            let reg = self.object_registry.lock().unwrap();
            reg.iter().map(|(id, r)| (*id, r.clone())).collect()
        };
        records.sort_by_key(|(id, _)| *id);
        let h2 = Self::hash_each(&records, |(id, r), buf| {
            buf.extend_from_slice(&id.to_le_bytes());
            buf.push(r.state as u8);
            buf.push(r.object_type as u8);
            // ObjectBody 不进入 — Memory 内容属于 StateStore
        });

        // 3. Topology — (from, to, link_type) 升序
        let mut edges = {
            let topo = self.topology.lock().unwrap();
            topo.clone()
        };
        edges.sort_by(|a, b| {
            a.from.cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then((a.link_type as u8).cmp(&(b.link_type as u8)))
        });
        
        let h3 = Self::hash_each(&edges, |e, buf| {
            buf.extend_from_slice(&e.from.to_le_bytes());
            buf.extend_from_slice(&e.to.to_le_bytes());
            buf.push(e.link_type as u8);
        });

        // 4. CapabilityGraph — 语义内容排序（不含 CapabilityId）
        let mut grants: Vec<(ObjectId, ObjectId, ObjectId, String)> = {
            let cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.all_grants()
                .into_iter()
                .map(|(_, info)| {
                    (info.granted_by, info.root_holder, info.resource, info.capability_type)
                })
                .collect()
        };
        grants.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
                .then(a.3.cmp(&b.3))
        });
        let h4 = Self::hash_each(&grants, |g, buf| {
            buf.extend_from_slice(&g.0.to_le_bytes());
            buf.extend_from_slice(&g.1.to_le_bytes());
            buf.extend_from_slice(&g.2.to_le_bytes());
            buf.extend_from_slice(g.3.as_bytes());
        });

        // 5. ScopeRegistry — ScopeId 升序，members 内部也排序
        let mut scopes = self.scope_registry.all_scopes();
        scopes.sort_by_key(|(id, _)| *id);
        let h5 = Self::hash_each(&scopes, |(id, entry), buf| {
            buf.extend_from_slice(&id.to_le_bytes());
            let mut members = entry.members.clone();
            members.sort();
            for m in &members {
                buf.extend_from_slice(&m.to_le_bytes());
            }
            buf.extend_from_slice(&entry.struct_version.to_le_bytes());
        });

        // 最终: H(h1, h2, h3, h4, h5)
        Self::hash_u64s(&[h1, h2, h3, h4, h5])
    }

    /// 调试用：返回五组件各自独立 hash，用于定位状态差异。
    #[allow(dead_code)]
    pub fn debug_root_components(&self) -> (u64, u64, u64, u64, u64) {
        // 与 root_hash() 中完全相同的计算，但不合并
        let mut entries = self.state_store.all_entries();
        entries.sort_by_key(|(addr, _)| *addr);
        let h1 = Self::hash_each(&entries, |(addr, entry), buf| {
            buf.extend_from_slice(&addr.object_id.to_le_bytes());
            buf.extend_from_slice(&addr.state_id.to_le_bytes());
            buf.extend_from_slice(&entry.value);
            buf.extend_from_slice(&entry.version.to_le_bytes());
        });

        let mut records: Vec<(ObjectId, crate::types::ObjectRecord)> = {
            let reg = self.object_registry.lock().unwrap();
            reg.iter().map(|(id, r)| (*id, r.clone())).collect()
        };
        records.sort_by_key(|(id, _)| *id);
        let h2 = Self::hash_each(&records, |(id, r), buf| {
            buf.extend_from_slice(&id.to_le_bytes());
            buf.push(r.state as u8);
            buf.push(r.object_type as u8);
        });

        let mut edges = {
            let topo = self.topology.lock().unwrap();
            topo.clone()
        };
        edges.sort_by(|a, b| {
            a.from.cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then((a.link_type as u8).cmp(&(b.link_type as u8)))
        });
        let h3 = Self::hash_each(&edges, |e, buf| {
            buf.extend_from_slice(&e.from.to_le_bytes());
            buf.extend_from_slice(&e.to.to_le_bytes());
            buf.push(e.link_type as u8);
        });

        // 4. CapabilityGraph — 语义内容排序（不含 CapabilityId）
        let mut grants: Vec<(ObjectId, ObjectId, ObjectId, String)> = {
            let cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.all_grants()
                .into_iter()
                .map(|(_, info)| {
                    (info.granted_by, info.root_holder, info.resource, info.capability_type)
                })
                .collect()
        };
        grants.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
                .then(a.3.cmp(&b.3))
        });
        let h4 = Self::hash_each(&grants, |g, buf| {
            buf.extend_from_slice(&g.0.to_le_bytes());
            buf.extend_from_slice(&g.1.to_le_bytes());
            buf.extend_from_slice(&g.2.to_le_bytes());
            buf.extend_from_slice(g.3.as_bytes());
        });

        let mut scopes = self.scope_registry.all_scopes();
        scopes.sort_by_key(|(id, _)| *id);
        let h5 = Self::hash_each(&scopes, |(id, entry), buf| {
            buf.extend_from_slice(&id.to_le_bytes());
            let mut members = entry.members.clone();
            members.sort();
            for m in &members {
                buf.extend_from_slice(&m.to_le_bytes());
            }
            buf.extend_from_slice(&entry.struct_version.to_le_bytes());
        });

        (h1, h2, h3, h4, h5)
    }

    /// 创建完全空的引擎，不读 WAL，不恢复。
    /// 仅用于 Replay 和测试。
    pub(crate) fn empty() -> Self {
        use std::sync::atomic::AtomicU64;
        use std::sync::{Arc, Mutex};
        use crate::lock::LockManager;
        use crate::tx_manager::TransactionManager;
        use crate::controller::TransactionController;
        use crate::capability::CapabilityGraph;
                use crate::store::StateStore;
        use crate::scope_registry::ScopeRegistry;

        let tx_mgr = Arc::new(TransactionManager::with_start_id(1));
        let lock_mgr = Arc::new(LockManager::new(Arc::clone(&tx_mgr)));
        let controller = TransactionController::new(Arc::clone(&tx_mgr), Arc::clone(&lock_mgr));

        VeritasEngine {
            global_version: AtomicU64::new(0),
            state_store: StateStore::new(),
            scope_registry: ScopeRegistry::new(),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open("target/replay_dummy.wal").expect("dummy WAL"),
            wal_path: String::new(),
            object_registry: Mutex::new(HashMap::new()),
            topology: Mutex::new(Vec::new()),
            capability_graph: Mutex::new(CapabilityGraph::new()),
            tx_mgr,
            lock_mgr,
            controller,
            object_id_counter: AtomicU64::new(1),
            last_dep_inv: Mutex::new(Vec::new()),
        }
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

        let (_, _, pending_effects, max_tx_id) =
            RecoveryManager::apply_records(&records);

        let tx_mgr = Arc::new(TransactionManager::with_start_id(max_tx_id + 1));
        let lock_mgr = Arc::new(LockManager::new(Arc::clone(&tx_mgr)));
        let controller = TransactionController::new(Arc::clone(&tx_mgr), Arc::clone(&lock_mgr));

        // Step 2c: 从 WAL records 按 tx_id 分组构建 TransactionDelta 列表
        // 只保留有 Commit marker 的事务，丢弃孤儿条目
        let ordered_deltas = crate::wal::build_ordered_deltas(&records);

        // Step 3/ObjectId: 从所有已提交事务中取 max(birth_id) 作为计数器起点
        let max_birth_id = ordered_deltas
            .iter()
            .flat_map(|d| d.births.iter())
            .max()
            .copied()
            .unwrap_or(0);
        let next_object_id = max_birth_id + 1;

        let engine = VeritasEngine {
            global_version: AtomicU64::new(recovered_version),
            state_store: StateStore::new(),
            scope_registry: ScopeRegistry::new(),
            commit_lock: Mutex::new(()),
            wal: WalWriter::open(&wal_path).expect("Failed to open WAL file"),
            wal_path: wal_path.clone(),
            object_registry: Mutex::new(HashMap::new()),
            topology: Mutex::new(Vec::new()),
            capability_graph: Mutex::new(CapabilityGraph::new()),
            tx_mgr,
            lock_mgr,
            controller,
            object_id_counter: AtomicU64::new(next_object_id),
            last_dep_inv: std::sync::Mutex::new(Vec::new()),
        };

        // Step 2c: 按 Commit 顺序依次 apply 每个 Delta
        for delta in &ordered_deltas {
            engine.apply(delta);
        }

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

    pub(crate) fn begin(&self) -> TransactionContext {
        let snapshot_version = self.global_version.load(Ordering::Acquire);
        self.controller.begin(snapshot_version)
    }




    pub(crate) fn begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
        let mut ctx = self.begin();
        ctx.enter_object(object_id);
        ctx
    }
    pub(crate) fn read(
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

    pub(crate) fn write(
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

    pub(crate) fn effect(
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

    pub(crate) fn commit(&self, ctx: &mut TransactionContext) -> Result<TransactionReceipt, VeritasError> {
        self.controller.pre_commit_check(ctx)?;

        let _lock = self.commit_lock.lock().unwrap();

        // Stage 3.3: 捕获 commit 前 WorldState 根哈希
        let before_root = self.root_hash();

        self.detect_conflict(ctx)?;
        self.detect_scope_conflict(ctx)?;
        self.verify_capability(ctx)?;

        // Step 1b: 验证 pending_links —— frozen 对象拒绝新 Link
        {
            let reg = self.object_registry.lock().unwrap();
            for edge in &ctx.pending_links {
                let from_frozen = reg.get(&edge.from).map(|r| r.is_frozen()).unwrap_or(false);
                let to_frozen = reg.get(&edge.to).map(|r| r.is_frozen()).unwrap_or(false);
                if from_frozen || to_frozen {
                    return Err(VeritasError::Abort(AbortReason::WriteConflict));
                }
            }
        }

        // Step 1b: 保存原始 deaths（OWNS 展开前），供后续 build_delta 使用
        let requested_deaths = ctx.pending_deaths.clone();

        let commit_version = self.global_version.load(Ordering::Acquire) + 1;

        let pending_effects = ctx.effect_queue.drain();

        // P8.1: OWNS 死亡闭包——from 死亡则 owned 对象一并进入 pending_deaths
        self.expand_owns_death_closure(ctx);

        // Step 3: build TransactionDelta and write as single atomic WAL entry
        let delta = self.build_delta(ctx, requested_deaths.clone(), commit_version);

        // drain pending collections (already cloned by build_delta)
        let _ = ctx.pending_capabilities.drain(..).collect::<Vec<_>>();
        let _ = ctx.pending_capability_revokes.drain(..).collect::<Vec<_>>();
        let _ = ctx.pending_freezes.drain(..).collect::<Vec<_>>();
        let wal_entry = WalEntry::TransactionCommitted(delta.clone());
        self.wal
            .append_and_sync(&wal_entry)
            .map_err(|e| VeritasError::EngineError(format!("WAL write failed: {}", e)))?;

        // Step 2b/Step 3: apply after WAL is durable
        self.apply(&delta);

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

        let after_root = self.root_hash();
        let receipt = TransactionReceipt {
            tx_id: delta.tx_id,
            before_root,
            delta,
            after_root,
        };

        Ok(receipt)
    }

    /// Step 1b: 从 TransactionContext 构建 TransactionDelta。
    ///
    /// 只提取原始事实，不做任何派生计算。
    /// deaths 使用 OWNS 展开**之前**的原始请求集合。
    pub fn build_delta(
        &self,
        ctx: &TransactionContext,
        requested_deaths: Vec<ObjectId>,
        commit_version: Version,
    ) -> TransactionDelta {
        let writes: Vec<(Address, Vec<u8>)> = ctx
            .write_set
            .iter()
            .map(|(addr, val)| (*addr, val.clone()))
            .collect();

        let scope_changes: Vec<(ScopeId, ScopeChangeType, StateId)> = ctx
            .scope_write_set
            .iter()
            .map(|c| (c.scope_id, c.change_type.clone(), c.state_id))
            .collect();

        let effects: Vec<(String, Vec<u8>)> = ctx
            .effect_queue
            .effects
            .iter()
            .map(|e| (e.idempotency_key.clone(), e.payload.clone()))
            .collect();

        let links: Vec<(ObjectId, ObjectId, LinkType)> = ctx
            .pending_links
            .iter()
            .map(|e| (e.from, e.to, e.link_type))
            .collect();

        let unlinks: Vec<(ObjectId, ObjectId)> = ctx.pending_unlinks.clone();

        TransactionDelta {
            tx_id: ctx.tx_id(),
            commit_version,
            writes,
            scope_changes,
            births: ctx.pending_objects.clone(),
            deaths: requested_deaths,
            freezes: ctx.pending_freezes.clone(),
            links,
            unlinks,
            capability_grants: ctx.pending_capabilities.clone(),
            capability_revokes: ctx.pending_capability_revokes.clone(),
            effects,
        }
    }

    /// Step 2a: apply() — 将 TransactionDelta 投影到所有内存结构。
    ///
    /// 这是 Runtime commit 和 Recovery replay 的唯一入口。
    /// 步骤顺序不可变更：links/unlinks 必须先于 OWNS 闭包展开，
    /// 否则闭包会漏算本事务内新增的 OWNS 边。
    pub fn apply(&self, delta: &TransactionDelta) {
        // 1. State writes
        for (addr, value) in &delta.writes {
            self.state_store.insert(
                *addr,
                StateEntry {
                    value: value.clone(),
                    version: delta.commit_version,
                },
            );
        }

        // 2. Scope changes
        for (scope_id, change_type, state_id) in &delta.scope_changes {
            match change_type {
                ScopeChangeType::Bind => {
                    self.scope_registry.apply_bind(*scope_id, *state_id);
                }
                ScopeChangeType::Unbind => {
                    self.scope_registry.apply_unbind(*scope_id, *state_id);
                }
            }
        }

        // 3. Births
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &delta.births {
                registry.insert(*object_id, crate::types::ObjectRecord::new_state(*object_id));
            }
        }

        // 4. Capability grants
        {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            for grant in &delta.capability_grants {
                cap_graph.restore_grant(
                    grant.capability_id,
                    grant.cap_type.clone(),
                    grant.grantor,
                    grant.grantee,
                    grant.resource,
                    grant.grant_sequence,
                );
            }
        }

        // 4b. Capability revokes (after grants so same-tx grant→revoke is ordered)
        {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            for rev in &delta.capability_revokes {
                // Ignore NotHolder on replay edge cases; commit path validates.
                let _ = cap_graph.revoke(rev.capability_id, rev.holder, rev.cascade_override);
            }
        }

        // 5. Links / Unlinks — must happen before OWNS closure (step 6)
        {
            let mut topo = self.topology.lock().unwrap();
            for (from, to, link_type) in &delta.links {
                topo.push(LinkEdge {
                    from: *from,
                    to: *to,
                    link_type: *link_type,
                });
            }
            for (from, to) in &delta.unlinks {
                topo.retain(|e| e.from != *from || e.to != *to);
            }
        }

        // 6. OWNS death closure: expand from delta.deaths using committed topology
        let full_death_set: HashSet<ObjectId> = {
            let topo = self.topology.lock().unwrap();
            let mut queue: Vec<ObjectId> = delta.deaths.clone();
            let mut seen: HashSet<ObjectId> = queue.iter().copied().collect();
            let mut i = 0;
            while i < queue.len() {
                let id = queue[i];
                for edge in topo.iter() {
                    if edge.from == id
                        && edge.link_type == LinkType::Owns
                        && seen.insert(edge.to)
                    {
                        queue.push(edge.to);
                    }
                }
                i += 1;
            }
            seen
        };

        // 7. DEPENDS_ON → DependencyInvalidated
        {
            let topo = self.topology.lock().unwrap();
            let mut inv: Vec<(ObjectId, ObjectId)> = Vec::new();
            for edge in topo.iter() {
                if edge.link_type == LinkType::DependsOn
                    && full_death_set.contains(&edge.to)
                    && !full_death_set.contains(&edge.from)
                {
                    inv.push((edge.from, edge.to));
                }
            }
            inv.sort_unstable();
            inv.dedup();
            *self.last_dep_inv.lock().unwrap() = inv;
        }

        // 8. Topology cleanup: remove edges involving dead objects
        {
            let mut topo = self.topology.lock().unwrap();
            topo.retain(|edge| !full_death_set.contains(&edge.from) && !full_death_set.contains(&edge.to));
        }

        // 9. Capability revoke for dead holders
        {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            for &dead_obj in &full_death_set {
                cap_graph.revoke_holder(dead_obj);
            }
        }

        // 10. Object registry: update death/freeze status
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &full_death_set {
                if let Some(r) = registry.get_mut(object_id) {
                    r.state = crate::types::ObjectState::Dead;
                } else {
                    let mut r = crate::types::ObjectRecord::new_state(*object_id);
                    r.state = crate::types::ObjectState::Dead;
                    registry.insert(*object_id, r);
                }
            }
            for object_id in &delta.freezes {
                if let Some(r) = registry.get_mut(object_id) {
                    r.state = crate::types::ObjectState::Frozen;
                } else {
                    let mut r = crate::types::ObjectRecord::new_state(*object_id);
                    r.state = crate::types::ObjectState::Frozen;
                    registry.insert(*object_id, r);
                }
            }
        }

        // 11. StateStore cleanup: remove all entries for dead objects
        for dead_obj in &full_death_set {
            self.state_store.remove_object(*dead_obj);
        }

        // Global version update
        self.global_version.store(delta.commit_version, Ordering::SeqCst);
    }

    /// P8.3: CAPABILITY_GRANT 原语——向 Alive 的 Object 授权
    pub(crate) fn capability_grant(
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

        let seq = {
            let cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.current_sequence() + 1 + ctx.pending_capabilities.len() as u64
        };
        let cap_id = crate::capability::capability_id_of(grantee, grantee, resource, seq);

        ctx.pending_capabilities.push(crate::types::PendingCapabilityGrant {
            capability_id: cap_id,
            grant_sequence: seq,
            grantor: grantee,
            grantee,
            resource,
            cap_type: capability_type.to_string(),
        });
        Ok(cap_id)
    }

    /// CAPABILITY_REVOKE: record a pending revoke; applied on commit via apply().
    /// Distinct from Death→revoke_holder. Uses CapabilityGraph::revoke (no reimplementation).
    pub(crate) fn capability_revoke(
        &self,
        ctx: &mut TransactionContext,
        capability_id: CapabilityId,
        holder: ObjectId,
        cascade_override: Option<bool>,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // Same-tx grant not yet applied: allow revoke of a pending grant by
        // removing it from pending_capabilities when holder is the grantee.
        let pending_idx = ctx
            .pending_capabilities
            .iter()
            .position(|g| g.capability_id == capability_id && g.grantee == holder);
        if let Some(i) = pending_idx {
            ctx.pending_capabilities.remove(i);
            // Also drop any pending revokes that targeted the removed grant edge.
            return Ok(());
        }

        // Committed graph must currently hold the capability.
        {
            let cap_graph = self.capability_graph.lock().unwrap();
            if !cap_graph.holds(capability_id, holder) {
                return Err(VeritasError::EngineError(format!(
                    "CapabilityRevoke: holder {} does not hold capability {}",
                    holder, capability_id
                )));
            }
        }

        ctx.pending_capability_revokes.push(crate::types::PendingCapabilityRevoke {
            capability_id,
            holder,
            cascade_override,
        });
        Ok(())
    }

    /// Test/engine helper: live delegate on the committed graph (not WAL-recorded).
    /// Used to build delegate trees for revoke integration tests until
    /// KernelCall::CapabilityDelegate exists.
    pub fn capability_delegate(
        &self,
        capability_id: CapabilityId,
        from: ObjectId,
        to: ObjectId,
        cascade_on_revoke: bool,
    ) -> Result<(), VeritasError> {
        let mut cap_graph = self.capability_graph.lock().unwrap();
        cap_graph
            .delegate(capability_id, from, to, cascade_on_revoke)
            .map_err(|e| VeritasError::EngineError(format!("delegate failed: {:?}", e)))
    }

    /// P26: OBJECT_FREEZE - 冻结Object，使其变为只读
    pub(crate) fn object_freeze(
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
    pub(crate) fn object_death(
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
    pub(crate) fn object_link(
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
    pub(crate) fn object_unlink(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }

        // P5.x.1: 检查边是否存在（全局 topology 或当前事务 pending_links）
        let topo = self.topology.lock().unwrap();
        let exists_in_topo = topo.iter().any(|e| e.from == from && e.to == to);
        let exists_in_pending = ctx.pending_links.iter().any(|e| e.from == from && e.to == to);
        drop(topo);

        if !exists_in_topo && !exists_in_pending {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        // 标记Link待删除，Commit时生效
        ctx.pending_unlinks.push((from, to));
        Ok(())
    }

    /// P4: OBJECT_BIRTH 最小物理原语
    pub(crate) fn object_birth(
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
        let (admin_cap_id, admin_seq) = {
            let cap_graph = self.capability_graph.lock().unwrap();
            let seq = cap_graph.current_sequence() + 1 + ctx.pending_capabilities.len() as u64;
            let id = crate::capability::capability_id_of(object_id, object_id, object_id, seq);
            (id, seq)
        };

        ctx.pending_capabilities.push(crate::types::PendingCapabilityGrant {
            capability_id: admin_cap_id,
            grant_sequence: admin_seq,
            cap_type: "AdminCap".into(),
            grantor: object_id,
            grantee: object_id,
            resource: object_id,
        });

        Ok(())
    }

    pub(crate) fn abort(&self, ctx: &mut TransactionContext, reason: AbortReason) {
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

    /// Step 3/ObjectId: 返回下一个可用的 ObjectId。
    /// 从已提交的 TransactionCommitted 条目中 max(birth_id)+1 起步，
    /// 单调递增。调用者不能指定 ID，只能从 Kernel 分配。
    pub fn next_object_id(&self) -> ObjectId {
        self.object_id_counter.fetch_add(1, Ordering::SeqCst)
    }

    pub fn get_global_version(&self) -> Version {
        self.global_version.load(Ordering::Acquire)
    }

    /// 返回 WAL 文件路径（用于 Replay 测试）
    pub fn wal_path(&self) -> &str {
        &self.wal_path
    }

    pub(crate) fn savepoint(
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
            pending_capabilities_len: ctx.pending_capabilities.len(),
            pending_capability_revokes_len: ctx.pending_capability_revokes.len(),
            pending_calls_len: ctx.pending_calls.len(),
        });

        Ok(())
    }

    pub(crate) fn rollback_to(
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
        ctx.pending_capabilities.truncate(sp.pending_capabilities_len);
        ctx.pending_capability_revokes.truncate(sp.pending_capability_revokes_len);
        ctx.pending_calls.truncate(sp.pending_calls_len);

        ctx.savepoints.truncate(index + 1);

        Ok(())
    }
}

