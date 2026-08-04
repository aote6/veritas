// Veritas Kernel V0.2 - 核心类型定义

use std::collections::HashMap;

pub type Version = u64;
pub type StateId = u64;

/// 二元寻址：Veritas中一切可访问的Memory位置都通过(ObjectId, StateId)定位。
/// 不存在脱离Object上下文的裸StateId访问——这是Memory宪法(memory.md)第4节的要求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address {
    pub object_id: ObjectId,
    pub state_id: StateId,
}

impl Address {
    pub fn new(object_id: ObjectId, state_id: StateId) -> Self {
        Address { object_id, state_id }
    }
}
pub type ScopeId = u64;
pub type TxId = u64;
pub type ModuleId = u64;
pub type CapabilityId = u64;

#[derive(Debug, Clone)]
pub struct StateEntry {
    pub value: Vec<u8>,
    pub version: Version,
}

// ============ Scope 类型（新增） ============

#[derive(Debug, Clone)]
pub struct ScopeEntry {
    pub members: Vec<StateId>,
    pub struct_version: Version,
    pub owner: ModuleId,
}

impl ScopeEntry {
    pub fn new() -> Self {
        ScopeEntry {
            members: Vec::new(),
            struct_version: 0,
            owner: 0,
        }
    }

    /// 返回 true 表示真的发生了变化
    pub fn bind(&mut self, state: StateId) -> bool {
        if self.members.contains(&state) {
            false
        } else {
            self.members.push(state);
            self.struct_version += 1;
            true
        }
    }

    pub fn unbind(&mut self, state: StateId) -> bool {
        if let Some(pos) = self.members.iter().position(|s| *s == state) {
            self.members.remove(pos);
            self.struct_version += 1;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeChangeType {
    Bind,
    Unbind,
}

#[derive(Debug, Clone)]
pub struct ScopeChange {
    pub scope_id: ScopeId,
    pub state_id: StateId,
    pub change_type: ScopeChangeType,
}

// ============ 事务核心结构 ============

// Runtime Object 基础类型
pub type ObjectId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectState {
    Alive,
    Frozen,
    Dead,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    DependsOn = 0,
    Owns = 1,
    References = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    pub from: ObjectId,
    pub to: ObjectId,
    pub link_type: LinkType,
}


#[derive(Debug, Clone, Default)]
pub struct ReadSet {
    pub states: HashMap<Address, Version>,
    pub scopes: HashMap<ScopeId, Version>,
}

#[derive(Debug, Clone, Default)]
pub struct WriteSet {
    /// 按写入顺序存储，支持回滚到任意 savepoint
    pub changes: Vec<(Address, Vec<u8>)>,
}

impl WriteSet {
    pub fn push(&mut self, addr: Address, value: Vec<u8>) {
        self.changes.push((addr, value));
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for (addr, data) in &self.changes {
            h ^= addr.state_id;
            h = h.wrapping_mul(0x100000001b3);
            for &b in data {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    pub fn get_latest(&self, addr: Address) -> Option<&Vec<u8>> {
        self.changes
            .iter()
            .rev()
            .find(|(a, _)| *a == addr)
            .map(|(_, val)| val)
    }

    pub fn contains_key(&self, addr: Address) -> bool {
        self.changes.iter().any(|(a, _)| *a == addr)
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn truncate(&mut self, len: usize) {
        self.changes.truncate(len);
    }

    pub fn keys(&self) -> Vec<Address> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for (addr, _) in &self.changes {
            if !seen.contains(addr) {
                seen.insert(*addr);
                result.push(*addr);
            }
        }
        result
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (Address, Vec<u8>)> {
        self.changes.iter()
    }
}

#[derive(Debug, Clone, Default)]
pub struct EffectQueue {
    pub effects: Vec<PendingEffect>,
}

impl EffectQueue {
    pub fn push(&mut self, effect: PendingEffect) {
        self.effects.push(effect);
    }

    pub fn drain(&mut self) -> Vec<PendingEffect> {
        std::mem::take(&mut self.effects)
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn truncate(&mut self, len: usize) {
        self.effects.truncate(len);
    }
}

#[derive(Debug, Clone)]
pub struct Savepoint {
    pub name: String,
    pub write_set_len: usize,
    pub effect_queue_len: usize,
    pub scope_write_set_len: usize,
    pub pending_objects_len: usize,
    pub pending_links_len: usize,
    pub pending_deaths_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCapabilityGrant {
    pub capability_id: u64,
    pub grant_sequence: u64,
    pub cap_type: String,
    pub grantor: ObjectId,
    pub grantee: ObjectId,
    pub resource: ObjectId,
}

pub struct TransactionContext {
    pub capabilities: Vec<u64>,
    pub program_hash: Option<u64>,
    pub tx_id: TxId,
    pub snapshot_version: Version,
    pub read_set: ReadSet,
    pub write_set: WriteSet,
    pub scope_write_set: Vec<ScopeChange>,
    pub effect_queue: EffectQueue,
    pub savepoints: Vec<Savepoint>,
    pub pending_links: Vec<LinkEdge>,
    pub pending_unlinks: Vec<(ObjectId, ObjectId)>,
    pub pending_freezes: Vec<ObjectId>,
    pub pending_deaths: Vec<ObjectId>,
    pub pending_objects: Vec<ObjectId>,
    pub pending_capabilities: Vec<PendingCapabilityGrant>,
    pub aborted: bool,
    /// 当前执行上下文所属的Object。一切Read/Write在没有显式CALL切换的情况下，
    /// 隐式作用于这个Object的Memory Space——这是Memory宪法(memory.md)第4节
    /// "地址 = (ObjectId, StateId)"在执行层的落地方式：地址的ObjectId分量
    /// 来自当前上下文，不需要每条指令自己携带。
    /// 当前 Transaction 内进行 Capability 检查时所使用的执行身份。
    /// CALL 时切换为目标 Object，RETURN 时从 CallFrame 恢复。
    pub capability_context: ObjectId,
    pub current_object: ObjectId,
}

impl TransactionContext {
    pub fn new(tx_id: TxId, snapshot_version: Version) -> Self {
        TransactionContext {
            capabilities: Vec::new(),
            program_hash: None,
            tx_id,
            snapshot_version,
            read_set: ReadSet::default(),
            write_set: WriteSet::default(),
            scope_write_set: Vec::new(),
            effect_queue: EffectQueue::default(),
            savepoints: Vec::new(),
            pending_objects: Vec::new(),
            pending_capabilities: Vec::new(),
            pending_links: Vec::new(),
            pending_unlinks: Vec::new(),
            pending_freezes: Vec::new(),
            pending_deaths: Vec::new(),
            aborted: false,
            capability_context: 0,
            current_object: 0,
        }
    }


    /// 切换当前执行上下文到另一个Object。对应CALL指令跨Module调用时
    /// 的语义(module.md第6节)：Machine切换执行上下文到被调用Object的
    /// 代码/内存空间。
    pub fn enter_object(&mut self, object_id: ObjectId) {
        self.current_object = object_id;
    }

    pub fn set_aborted(&mut self) {
        self.aborted = true;
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    pub fn snapshot_version(&self) -> Version {
        self.snapshot_version
    }

    pub fn clear(&mut self) {
        self.read_set.states.clear();
        self.read_set.scopes.clear();
        self.write_set.changes.clear();
        self.scope_write_set.clear();
        self.effect_queue = EffectQueue::default();
        self.savepoints.clear();
        self.aborted = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    WriteConflict,
    ReadFutureVersion,
    AlreadyAborted,
    StateNotFound,
    PhantomConflict,
}

impl std::fmt::Display for AbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbortReason::WriteConflict => write!(f, "Write conflict detected"),
            AbortReason::ReadFutureVersion => write!(f, "Read future version"),
            AbortReason::AlreadyAborted => write!(f, "Transaction already aborted"),
            AbortReason::StateNotFound => write!(f, "State not found"),
            AbortReason::PhantomConflict => write!(f, "Scope structure changed (phantom read)"),
        }
    }
}

#[derive(Debug)]
pub enum VeritasError {
    Abort(AbortReason),
    EngineError(String),
    PermissionDenied,
    DeterminismViolation,
}

impl From<AbortReason> for VeritasError {
    fn from(reason: AbortReason) -> Self {
        VeritasError::Abort(reason)
    }
}

pub fn deterministic_hash(input: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 待执行的副作用（暂存，尚未真正执行）
#[derive(Debug, Clone)]
pub struct PendingEffect {
    pub idempotency_key: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrapReason {
    InvalidOpcode { opcode: u8 },
    InvalidEncoding { pc: usize },
    MemoryFault { addr: usize, size: usize },
    DivisionByZero,
    ArithmeticOverflow,
    IllegalInstruction { opcode: u8 },
    /// P29: Capability检查失败，硬件级越权拦截
    AccessDenied { pc: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrapFrame {
    pub pc: usize,
    pub reason: TrapReason,
    pub cycles: u64,
}


impl ObjectState {
    #[inline]
    pub fn is_alive(self) -> bool {
        matches!(self, ObjectState::Alive)
    }

    #[inline]
    pub fn is_frozen(self) -> bool {
        matches!(self, ObjectState::Frozen)
    }

    #[inline]
    pub fn is_dead(self) -> bool {
        matches!(self, ObjectState::Dead)
    }
}

// =================================================================
// Veritas Constitution Alignment: Object Specification v0.2 (P30)
// =================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectType {
    StateObject,
    ModuleObject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRule {
    pub max_instances: Option<u32>,
    pub allow_instructions: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectBody {
    State,
    Module {
        code_section: Vec<u8>,
        import_section: Vec<ObjectId>,
        export_section: std::collections::HashMap<String, usize>,
        verification_rule: Option<VerificationRule>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRecord {
    pub id: ObjectId,
    pub object_type: ObjectType,
    pub state: ObjectState,
    pub body: ObjectBody,
}

impl ObjectRecord {
    pub fn new_state(id: ObjectId) -> Self {
        Self {
            id,
            object_type: ObjectType::StateObject,
            state: ObjectState::Alive,
            body: ObjectBody::State,
        }
    }

    pub fn new_module(
        id: ObjectId,
        code_section: Vec<u8>,
        import_section: Vec<ObjectId>,
        export_section: std::collections::HashMap<String, usize>,
        verification_rule: Option<VerificationRule>,
    ) -> Self {
        Self {
            id,
            object_type: ObjectType::ModuleObject,
            state: ObjectState::Frozen,
            body: ObjectBody::Module {
                code_section,
                import_section,
                export_section,
                verification_rule,
            },
        }
    }

    #[inline]
    pub fn is_alive(&self) -> bool {
        self.state.is_alive()
    }

    #[inline]
    pub fn is_frozen(&self) -> bool {
        self.state.is_frozen()
    }

    #[inline]
    pub fn is_dead(&self) -> bool {
        self.state.is_dead()
    }


}

/// P30 Step 1b: TransactionDelta — 一次事务的全部副作用集合。
///
/// WAL 原子写入单元。apply() 的唯一输入。
/// 所有字段都是"原始请求"，不含派生结果：
/// - deaths 仅为用户显式请求死亡的对象，不含 OWNS 级联展开
///   （apply() 内部需自行调用 expand_owns_death_closure）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionDelta {
    pub tx_id: TxId,
    pub commit_version: Version,

    // State
    pub writes: Vec<(Address, Vec<u8>)>,

    // Scope
    pub scope_changes: Vec<(ScopeId, ScopeChangeType, StateId)>,

    // Object lifecycle (原始请求，非展开后)
    pub births: Vec<ObjectId>,
    pub deaths: Vec<ObjectId>,    // 仅用户显式请求，apply() 内部展开 OWNS
    pub freezes: Vec<ObjectId>,

    // Topology
    pub links: Vec<(ObjectId, ObjectId, LinkType)>,
    pub unlinks: Vec<(ObjectId, ObjectId)>,

    // Capability
    pub capability_grants: Vec<PendingCapabilityGrant>,

    // Effects (待执行)
    pub effects: Vec<(String, Vec<u8>)>,  // (idempotency_key, payload)
}

/// Capability 语义快照记录（进入 Commitment Domain）。
/// 不含 CapabilityId——该 ID 由恢复时重新生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySemanticRecord {
    pub granted_by: ObjectId,
    pub holder: ObjectId,
    pub resource: ObjectId,
    pub capability_type: String,
    pub active: bool,
}

/// WorldState 完整快照。
/// Commitment Domain 部分进入 root_hash；
/// Continuation Metadata 不进入 root_hash。


/// TransactionReceipt: 一次 commit 的状态转移证明。
///
/// 证明：给定 before_root 和 TransactionDelta，
/// 经过 apply() 后必然得到 after_root。
/// 验证者需要拥有 before WorldState 才能验证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionReceipt {
    pub tx_id: TxId,
    pub before_root: u64,
    pub delta: TransactionDelta,
    pub after_root: u64,
}

impl TransactionDelta {
    /// Serialize to a deterministic text format for WAL storage.
    /// Format:
    ///   TXCOMMIT TX=<id> VERSION=<v>
    ///     WRITE <obj> <state> <hex_value>
    ///     SCOPE <scope_id> <BIND|UNBIND> <state_id>
    ///     BIRTH <obj>
    ///     DEATH <obj>
    ///     FREEZE <obj>
    ///     LINK <from> <to> <0|1|2>
    ///     UNLINK <from> <to>
    ///     CAPGRANT <cap_id> <seq> <grantor> <grantee> <resource> <type>
    ///     EFFECT <key> <hex_payload>
    ///     END
    pub fn serialize(&self) -> String {
        let mut s = format!("TXCOMMIT TX={} VERSION={}", self.tx_id, self.commit_version);
        for (addr, val) in &self.writes {
            s.push_str(&format!(
                " WRITE {} {} {}",
                addr.object_id, addr.state_id, hex::encode(val)
            ));
        }
        for (scope_id, change_type, state_id) in &self.scope_changes {
            let tag = match change_type {
                ScopeChangeType::Bind => "SCOPEBIND",
                ScopeChangeType::Unbind => "SCOPEUNBIND",
            };
            s.push_str(&format!(" {} {} {}", tag, scope_id, state_id));
        }
        for id in &self.births {
            s.push_str(&format!(" BIRTH {}", id));
        }
        for id in &self.deaths {
            s.push_str(&format!(" DEATH {}", id));
        }
        for id in &self.freezes {
            s.push_str(&format!(" FREEZE {}", id));
        }
        for (from, to, link_type) in &self.links {
            s.push_str(&format!(" LINK {} {} {}", from, to, *link_type as u8));
        }
        for (from, to) in &self.unlinks {
            s.push_str(&format!(" UNLINK {} {}", from, to));
        }
        for grant in &self.capability_grants {
            s.push_str(&format!(
                " CAPGRANT {} {} {} {} {} {}",
                grant.capability_id, grant.grant_sequence,
                grant.grantor, grant.grantee, grant.resource, grant.cap_type
            ));
        }
        for (key, payload) in &self.effects {
            s.push_str(&format!(" EFFECT {} {}", key, hex::encode(payload)));
        }
        s.push_str(" END");
        s
    }

    /// Deserialize from the text format produced by serialize().
    /// Returns None on any parse error.
    pub fn deserialize(payload: &str) -> Option<Self> {
        let parts: Vec<&str> = payload.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "TXCOMMIT" || parts.last() != Some(&"END") {
            return None;
        }
        let tx_id = parts.iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>().ok()?;
        let commit_version = parts.iter()
            .find(|p| p.starts_with("VERSION="))?
            .strip_prefix("VERSION=")?
            .parse::<Version>().ok()?;

        let mut writes = Vec::new();
        let mut scope_changes = Vec::new();
        let mut births = Vec::new();
        let mut deaths = Vec::new();
        let mut freezes = Vec::new();
        let mut links = Vec::new();
        let mut unlinks = Vec::new();
        let mut capability_grants = Vec::new();
        let mut effects = Vec::new();

        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "WRITE" if i + 3 < parts.len() => {
                    let obj = parts[i+1].parse::<ObjectId>().ok()?;
                    let state = parts[i+2].parse::<StateId>().ok()?;
                    let val = hex::decode(parts[i+3]).ok()?;
                    writes.push((Address::new(obj, state), val));
                    i += 4;
                }
                "SCOPEBIND" if i + 2 < parts.len() => {
                    let sid = parts[i+1].parse::<ScopeId>().ok()?;
                    let st = parts[i+2].parse::<StateId>().ok()?;
                    scope_changes.push((sid, ScopeChangeType::Bind, st));
                    i += 3;
                }
                "SCOPEUNBIND" if i + 2 < parts.len() => {
                    let sid = parts[i+1].parse::<ScopeId>().ok()?;
                    let st = parts[i+2].parse::<StateId>().ok()?;
                    scope_changes.push((sid, ScopeChangeType::Unbind, st));
                    i += 3;
                }
                "BIRTH" if i + 1 < parts.len() => {
                    births.push(parts[i+1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "DEATH" if i + 1 < parts.len() => {
                    deaths.push(parts[i+1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "FREEZE" if i + 1 < parts.len() => {
                    freezes.push(parts[i+1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "LINK" if i + 3 < parts.len() => {
                    let from = parts[i+1].parse::<ObjectId>().ok()?;
                    let to = parts[i+2].parse::<ObjectId>().ok()?;
                    let lt = parts[i+3].parse::<u8>().ok()?;
                    let link_type = match lt {
                        0 => LinkType::DependsOn,
                        1 => LinkType::Owns,
                        2 => LinkType::References,
                        _ => return None,
                    };
                    links.push((from, to, link_type));
                    i += 4;
                }
                "UNLINK" if i + 2 < parts.len() => {
                    let from = parts[i+1].parse::<ObjectId>().ok()?;
                    let to = parts[i+2].parse::<ObjectId>().ok()?;
                    unlinks.push((from, to));
                    i += 3;
                }
                "CAPGRANT" if i + 6 < parts.len() => {
                    let cap_id = parts[i+1].parse::<u64>().ok()?;
                    let seq = parts[i+2].parse::<u64>().ok()?;
                    let grantor = parts[i+3].parse::<ObjectId>().ok()?;
                    let grantee = parts[i+4].parse::<ObjectId>().ok()?;
                    let resource = parts[i+5].parse::<ObjectId>().ok()?;
                    let cap_type = parts[i+6].to_string();
                    capability_grants.push(PendingCapabilityGrant {
                        capability_id: cap_id,
                        grant_sequence: seq,
                        cap_type,
                        grantor,
                        grantee,
                        resource,
                    });
                    i += 7;
                }
                "EFFECT" if i + 2 < parts.len() => {
                    let key = parts[i+1].to_string();
                    let payload = hex::decode(parts[i+2]).ok()?;
                    effects.push((key, payload));
                    i += 3;
                }
                "TXCOMMIT" | "END" | _ if parts[i].starts_with("TX=") | parts[i].starts_with("VERSION=") => {
                    i += 1;
                }
                _ => i += 1,
            }
        }

        Some(TransactionDelta {
            tx_id,
            commit_version,
            writes,
            scope_changes,
            births,
            deaths,
            freezes,
            links,
            unlinks,
            capability_grants,
            effects,
        })
    }
}



/// WorldSnapshot 是 Stage 3.4b 规范的恢复协议（Serialization Contract）
/// 仅保存稳定的纯语义数据，绝对不与子模块的内部数据结构绑定。

/// WorldSnapshot 是 Stage 3.4b 规范的恢复协议（Serialization Contract）
/// 仅保存稳定的纯语义数据，绝对不与子模块内部内存实现强绑定。
/// 这是 Kernel 的持久化协议，不是运行时结构的镜像。
#[derive(Debug, Clone)]
pub struct WorldSnapshot {
    pub commitment_hash: [u8; 32],
    pub tx_id: u64,
    pub state_entries: Vec<(Address, Vec<u8>)>,
    pub capability_records: Vec<CapabilitySemanticRecord>,
    pub objects: Vec<ObjectSnapshot>,
    pub links: Vec<LinkSnapshot>,
    pub scopes: Vec<ScopeSnapshot>,
}

/// Object 的稳定语义快照。不绑定 ObjectRecord 内部布局。
#[derive(Debug, Clone)]
pub struct ObjectSnapshot {
    pub id: ObjectId,
    pub object_type: ObjectType,
    pub lifecycle_state: ObjectState,
    pub metadata: Vec<u8>,
    pub payload: Vec<u8>,
}

/// Link 的稳定语义快照。结构体形式支持未来扩展字段。
#[derive(Debug, Clone)]
pub struct LinkSnapshot {
    pub from: ObjectId,
    pub to: ObjectId,
    pub link_type: LinkType,
}

/// Scope 的稳定语义快照。owner 使用 ObjectId，不暴露 ModuleId。
#[derive(Debug, Clone)]
pub struct ScopeSnapshot {
    pub scope_id: ScopeId,
    pub members: Vec<StateId>,
    pub owner: ObjectId,
}
