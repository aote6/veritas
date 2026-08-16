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
        Address {
            object_id,
            state_id,
        }
    }
}
pub type ScopeId = u64;
pub type TxId = u64;
pub type ModuleId = u64;
pub type CapabilityId = u64;

#[derive(Debug, Clone, PartialEq)]
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
    pub pending_capabilities_len: usize,
    pub pending_capability_revokes_len: usize,
    pub pending_delegates_len: usize,
    pub pending_calls_len: usize,
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

/// Explicit CAPABILITY_REVOKE recorded in the transaction (distinct from Death→revoke_holder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCapabilityRevoke {
    pub capability_id: CapabilityId,
    pub holder: ObjectId,
    /// None → use the edge's cascade_on_revoke (root defaults to true).
    pub cascade_override: Option<bool>,
}

/// CAPABILITY_DELEGATE recorded in the transaction: add a holder edge under an existing capability.
/// Does not create a new CapabilityId or advance grant_sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCapabilityDelegate {
    pub capability_id: CapabilityId,
    pub from: ObjectId,
    pub to: ObjectId,
    pub cascade_on_revoke: bool,
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
    pub pending_capability_revokes: Vec<PendingCapabilityRevoke>,
    pub pending_delegates: Vec<PendingCapabilityDelegate>,
    /// Cross-object CALL targets attempted in this transaction (AccessIntent::Call).
    pub pending_calls: Vec<ObjectId>,
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
            pending_capability_revokes: Vec::new(),
            pending_delegates: Vec::new(),
            pending_calls: Vec::new(),
            pending_links: Vec::new(),
            pending_unlinks: Vec::new(),
            pending_freezes: Vec::new(),
            pending_deaths: Vec::new(),
            aborted: false,
            capability_context: 0,
            current_object: 0,
        }
    }

    /// Internal state-setting primitive: sets `current_object` only.
    ///
    /// - Does **not** modify `capability_context`.
    /// - Is **not** equivalent to Machine `CALL` (no authorize_intent, no CallFrame).
    /// - Is **not** a full execution-time identity switch.
    /// - Callers are responsible for authorization; production cross-object
    ///   Host paths must `authorize_intent` **before** calling this.
    /// - Must **not** be used as a new execution-time identity-switch path.
    ///
    /// See `docs/IDENTITY_MODEL.md` §7 (Session / Host Bootstrap).
    /// Historical note: module.md §6 described CALL-driven context switch;
    /// that full switch is implemented by Machine CALL, not by this primitive.
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
    InvalidOpcode {
        opcode: u8,
    },
    InvalidEncoding {
        pc: usize,
    },
    MemoryFault {
        addr: usize,
        size: usize,
    },
    DivisionByZero,
    ArithmeticOverflow,
    IllegalInstruction {
        opcode: u8,
    },
    /// P29: Capability检查失败，硬件级越权拦截
    AccessDenied {
        pc: usize,
    },
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
    pub actor_id: u64,

    // State
    pub writes: Vec<(Address, Vec<u8>)>,

    // Scope
    pub scope_changes: Vec<(ScopeId, ScopeChangeType, StateId)>,

    // Object lifecycle (原始请求，非展开后)
    pub births: Vec<ObjectId>,
    pub deaths: Vec<ObjectId>, // 仅用户显式请求，apply() 内部展开 OWNS
    pub freezes: Vec<ObjectId>,

    // Topology
    pub links: Vec<(ObjectId, ObjectId, LinkType)>,
    pub unlinks: Vec<(ObjectId, ObjectId)>,

    // Capability
    pub capability_grants: Vec<PendingCapabilityGrant>,
    pub capability_delegates: Vec<PendingCapabilityDelegate>,
    pub capability_revokes: Vec<PendingCapabilityRevoke>,

    // Effects (待执行)
    pub effects: Vec<(String, Vec<u8>)>, // (idempotency_key, payload)
}

/// Capability 语义快照记录（进入 Commitment Domain）。
/// capability_id 是稳定身份，必须随 snapshot 持久化，restore 时不得重新生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySemanticRecord {
    pub capability_id: CapabilityId,
    pub granted_by: ObjectId,
    pub holder: ObjectId,
    pub resource: ObjectId,
    pub capability_type: String,
    pub active: bool,
    pub parent: Option<ObjectId>,
    pub cascade_on_revoke: bool,
}

/// AccessIntent — commit 前所有跨 Object side-effect 的统一访问意图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessIntent {
    Read(ObjectId),
    Write(ObjectId),
    Link(ObjectId, ObjectId),
    Unlink(ObjectId, ObjectId),
    Destroy(ObjectId),
    Freeze(ObjectId),
    /// Cross-object CALL: enter another Object's execution context.
    Call(ObjectId),
}

impl AccessIntent {
    pub fn target_objects(&self) -> Vec<ObjectId> {
        match self {
            AccessIntent::Read(id)
            | AccessIntent::Write(id)
            | AccessIntent::Destroy(id)
            | AccessIntent::Freeze(id)
            | AccessIntent::Call(id) => vec![*id],
            AccessIntent::Link(from, to) => vec![*from, *to],
            AccessIntent::Unlink(from, _to) => vec![*from],
        }
    }
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
    /// Commitment algorithm version. 1 = SHA-256.
    pub commitment_algorithm: u8,
    pub before_root: [u8; 32],
    pub delta: TransactionDelta,
    pub after_root: [u8; 32],
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
        let mut s = format!(
            "TXCOMMIT TX={} VERSION={} ACTOR={}",
            self.tx_id, self.commit_version, self.actor_id
        );
        for (addr, val) in &self.writes {
            s.push_str(&format!(
                " WRITE {} {} {}",
                addr.object_id,
                addr.state_id,
                hex::encode(val)
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
                grant.capability_id,
                grant.grant_sequence,
                grant.grantor,
                grant.grantee,
                grant.resource,
                grant.cap_type
            ));
        }
        for d in &self.capability_delegates {
            s.push_str(&format!(
                " CAPDELEGATE {} {} {} {}",
                d.capability_id,
                d.from,
                d.to,
                if d.cascade_on_revoke { 1u8 } else { 0u8 }
            ));
        }
        for rev in &self.capability_revokes {
            let cas = match rev.cascade_override {
                None => 2u8,
                Some(true) => 1u8,
                Some(false) => 0u8,
            };
            s.push_str(&format!(
                " CAPREVOKE {} {} {}",
                rev.capability_id, rev.holder, cas
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
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let commit_version = parts
            .iter()
            .find(|p| p.starts_with("VERSION="))?
            .strip_prefix("VERSION=")?
            .parse::<Version>()
            .ok()?;
        let actor_id = parts
            .iter()
            .find(|p| p.starts_with("ACTOR="))
            .and_then(|p| p.strip_prefix("ACTOR="))
            .and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(0);

        let mut writes = Vec::new();
        let mut scope_changes = Vec::new();
        let mut births = Vec::new();
        let mut deaths = Vec::new();
        let mut freezes = Vec::new();
        let mut links = Vec::new();
        let mut unlinks = Vec::new();
        let mut capability_grants = Vec::new();
        let mut capability_delegates = Vec::new();
        let mut capability_revokes = Vec::new();
        let mut effects = Vec::new();

        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "WRITE" if i + 3 < parts.len() => {
                    let obj = parts[i + 1].parse::<ObjectId>().ok()?;
                    let state = parts[i + 2].parse::<StateId>().ok()?;
                    let val = hex::decode(parts[i + 3]).ok()?;
                    writes.push((Address::new(obj, state), val));
                    i += 4;
                }
                "SCOPEBIND" if i + 2 < parts.len() => {
                    let sid = parts[i + 1].parse::<ScopeId>().ok()?;
                    let st = parts[i + 2].parse::<StateId>().ok()?;
                    scope_changes.push((sid, ScopeChangeType::Bind, st));
                    i += 3;
                }
                "SCOPEUNBIND" if i + 2 < parts.len() => {
                    let sid = parts[i + 1].parse::<ScopeId>().ok()?;
                    let st = parts[i + 2].parse::<StateId>().ok()?;
                    scope_changes.push((sid, ScopeChangeType::Unbind, st));
                    i += 3;
                }
                "BIRTH" if i + 1 < parts.len() => {
                    births.push(parts[i + 1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "DEATH" if i + 1 < parts.len() => {
                    deaths.push(parts[i + 1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "FREEZE" if i + 1 < parts.len() => {
                    freezes.push(parts[i + 1].parse::<ObjectId>().ok()?);
                    i += 2;
                }
                "LINK" if i + 3 < parts.len() => {
                    let from = parts[i + 1].parse::<ObjectId>().ok()?;
                    let to = parts[i + 2].parse::<ObjectId>().ok()?;
                    let lt = parts[i + 3].parse::<u8>().ok()?;
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
                    let from = parts[i + 1].parse::<ObjectId>().ok()?;
                    let to = parts[i + 2].parse::<ObjectId>().ok()?;
                    unlinks.push((from, to));
                    i += 3;
                }
                "CAPGRANT" if i + 6 < parts.len() => {
                    let cap_id = parts[i + 1].parse::<u64>().ok()?;
                    let seq = parts[i + 2].parse::<u64>().ok()?;
                    let grantor = parts[i + 3].parse::<ObjectId>().ok()?;
                    let grantee = parts[i + 4].parse::<ObjectId>().ok()?;
                    let resource = parts[i + 5].parse::<ObjectId>().ok()?;
                    let cap_type = parts[i + 6].to_string();
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
                "CAPDELEGATE" if i + 4 < parts.len() => {
                    let cap_id = parts[i + 1].parse::<CapabilityId>().ok()?;
                    let from = parts[i + 2].parse::<ObjectId>().ok()?;
                    let to = parts[i + 3].parse::<ObjectId>().ok()?;
                    let cas = parts[i + 4].parse::<u8>().ok()?;
                    capability_delegates.push(PendingCapabilityDelegate {
                        capability_id: cap_id,
                        from,
                        to,
                        cascade_on_revoke: cas != 0,
                    });
                    i += 5;
                }
                "CAPREVOKE" if i + 3 < parts.len() => {
                    let cap_id = parts[i + 1].parse::<CapabilityId>().ok()?;
                    let holder = parts[i + 2].parse::<ObjectId>().ok()?;
                    let cas = parts[i + 3].parse::<u8>().ok()?;
                    let cascade_override = match cas {
                        0 => Some(false),
                        1 => Some(true),
                        _ => None,
                    };
                    capability_revokes.push(PendingCapabilityRevoke {
                        capability_id: cap_id,
                        holder,
                        cascade_override,
                    });
                    i += 4;
                }
                "EFFECT" if i + 2 < parts.len() => {
                    let key = parts[i + 1].to_string();
                    let payload = hex::decode(parts[i + 2]).ok()?;
                    effects.push((key, payload));
                    i += 3;
                }
                "TXCOMMIT" | "END" | _
                    if parts[i].starts_with("TX=")
                        | parts[i].starts_with("VERSION=")
                        | parts[i].starts_with("ACTOR=") =>
                {
                    i += 1;
                }
                _ => i += 1,
            }
        }

        Some(TransactionDelta {
            tx_id,
            commit_version,
            actor_id,
            writes,
            scope_changes,
            births,
            deaths,
            freezes,
            links,
            unlinks,
            capability_grants,
            capability_delegates,
            capability_revokes,
            effects,
        })
    }

    /// Canonical identity bytes for Delta Identity (constitution §3.3).
    ///
    /// Independent of WAL `serialize()`. Deterministic, boundary-safe binary
    /// encoding. Excludes `tx_id` and `commit_version`. Includes `actor_id`
    /// and every semantic mutation field, preserving Vec order.
    ///
    /// Encoding rules (frozen constitution §3.3):
    /// - integers: fixed-width little-endian (u64 = 8 bytes)
    /// - strings / byte arrays: u64 length prefix + raw bytes
    /// - enums: explicit stable tags
    /// - collections: u64 count + elements in original order
    /// - field order: ACTOR, WRITES, SCOPE_CHANGES, BIRTHS, DEATHS, FREEZES,
    ///   LINKS, UNLINKS, CAPABILITY_GRANTS, CAPABILITY_DELEGATES,
    ///   CAPABILITY_REVOKES, EFFECTS
    pub fn canonical_identity_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // ACTOR
        buf.extend_from_slice(&self.actor_id.to_le_bytes());

        // WRITES: count + (object_id, state_id, value_len, value)*
        buf.extend_from_slice(&(self.writes.len() as u64).to_le_bytes());
        for (addr, val) in &self.writes {
            buf.extend_from_slice(&addr.object_id.to_le_bytes());
            buf.extend_from_slice(&addr.state_id.to_le_bytes());
            buf.extend_from_slice(&(val.len() as u64).to_le_bytes());
            buf.extend_from_slice(val);
        }

        // SCOPE_CHANGES: count + (scope_id, tag, state_id)*
        // tag: Bind=0, Unbind=1
        buf.extend_from_slice(&(self.scope_changes.len() as u64).to_le_bytes());
        for (scope_id, change_type, state_id) in &self.scope_changes {
            buf.extend_from_slice(&scope_id.to_le_bytes());
            let tag: u8 = match change_type {
                ScopeChangeType::Bind => 0,
                ScopeChangeType::Unbind => 1,
            };
            buf.push(tag);
            buf.extend_from_slice(&state_id.to_le_bytes());
        }

        // BIRTHS
        buf.extend_from_slice(&(self.births.len() as u64).to_le_bytes());
        for id in &self.births {
            buf.extend_from_slice(&id.to_le_bytes());
        }

        // DEATHS
        buf.extend_from_slice(&(self.deaths.len() as u64).to_le_bytes());
        for id in &self.deaths {
            buf.extend_from_slice(&id.to_le_bytes());
        }

        // FREEZES
        buf.extend_from_slice(&(self.freezes.len() as u64).to_le_bytes());
        for id in &self.freezes {
            buf.extend_from_slice(&id.to_le_bytes());
        }

        // LINKS: count + (from, to, link_type_u8)*
        buf.extend_from_slice(&(self.links.len() as u64).to_le_bytes());
        for (from, to, link_type) in &self.links {
            buf.extend_from_slice(&from.to_le_bytes());
            buf.extend_from_slice(&to.to_le_bytes());
            buf.push(*link_type as u8);
        }

        // UNLINKS
        buf.extend_from_slice(&(self.unlinks.len() as u64).to_le_bytes());
        for (from, to) in &self.unlinks {
            buf.extend_from_slice(&from.to_le_bytes());
            buf.extend_from_slice(&to.to_le_bytes());
        }

        // CAPABILITY_GRANTS
        buf.extend_from_slice(&(self.capability_grants.len() as u64).to_le_bytes());
        for g in &self.capability_grants {
            buf.extend_from_slice(&g.capability_id.to_le_bytes());
            buf.extend_from_slice(&g.grant_sequence.to_le_bytes());
            buf.extend_from_slice(&g.grantor.to_le_bytes());
            buf.extend_from_slice(&g.grantee.to_le_bytes());
            buf.extend_from_slice(&g.resource.to_le_bytes());
            let tb = g.cap_type.as_bytes();
            buf.extend_from_slice(&(tb.len() as u64).to_le_bytes());
            buf.extend_from_slice(tb);
        }

        // CAPABILITY_DELEGATES
        buf.extend_from_slice(&(self.capability_delegates.len() as u64).to_le_bytes());
        for d in &self.capability_delegates {
            buf.extend_from_slice(&d.capability_id.to_le_bytes());
            buf.extend_from_slice(&d.from.to_le_bytes());
            buf.extend_from_slice(&d.to.to_le_bytes());
            buf.push(if d.cascade_on_revoke { 1u8 } else { 0u8 });
        }

        // CAPABILITY_REVOKES
        // cascade_override: 0 = None, 1 = Some(true), 2 = Some(false)
        buf.extend_from_slice(&(self.capability_revokes.len() as u64).to_le_bytes());
        for r in &self.capability_revokes {
            buf.extend_from_slice(&r.capability_id.to_le_bytes());
            buf.extend_from_slice(&r.holder.to_le_bytes());
            let tag: u8 = match r.cascade_override {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            };
            buf.push(tag);
        }

        // EFFECTS: count + (key_len, key, payload_len, payload)*
        buf.extend_from_slice(&(self.effects.len() as u64).to_le_bytes());
        for (key, payload) in &self.effects {
            let kb = key.as_bytes();
            buf.extend_from_slice(&(kb.len() as u64).to_le_bytes());
            buf.extend_from_slice(kb);
            buf.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            buf.extend_from_slice(payload);
        }

        buf
    }

    /// Content hash of this Delta for identity comparison.
    /// Uses the same FNV-1a byte hash as the rest of the kernel; result is
    /// expanded to 32 bytes (first 8 = LE u64, remainder zero) to match
    /// the same [u8; 32] width as state_commitment.
    /// Note: content_hash still uses FNV-1a; algorithm migration is Phase 2D.
    pub fn content_hash(&self) -> [u8; 32] {
        delta_content_hash(&self.canonical_identity_bytes())
    }
}

/// Fixed zero hash for genesis / no-applied-delta (constitution §4).
pub const ZERO_HASH: [u8; 32] = [0u8; 32];

/// Hash canonical identity bytes into a 32-byte Hash value.
/// Reuses the kernel's existing FNV-1a over bytes (no new algorithm).
pub fn delta_content_hash(identity_bytes: &[u8]) -> [u8; 32] {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut h = FNV_OFFSET_BASIS;
    for &b in identity_bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    let mut out = [0u8; 32];
    out[0..8].copy_from_slice(&h.to_le_bytes());
    out
}

/// WorldSnapshot 是 Stage 3.4b 规范的恢复协议（Serialization Contract）
/// 仅保存稳定的纯语义数据，绝对不与子模块的内部数据结构绑定。

/// WorldSnapshot 是 Stage 3.4b 规范的恢复协议（Serialization Contract）
/// 仅保存稳定的纯语义数据，绝对不与子模块内部内存实现强绑定。
/// 这是 Kernel 的持久化协议，不是运行时结构的镜像。
#[derive(Debug, Clone)]
pub struct WorldSnapshot {
    /// Commitment algorithm version. 1 = SHA-256.
    /// Future algorithms get new version numbers; verification must
    /// read this field before interpreting state_commitment.
    pub commitment_algorithm: u8,
    /// State Identity / State Commitment: root_hash() over the five
    /// Commitment Domain components (StateStore, ObjectRegistry, Topology,
    /// CapabilityGraph, ScopeRegistry). Does not cover Continuation Metadata.
    pub state_commitment: [u8; 32],
    pub state_entries: Vec<(Address, StateEntry)>,
    pub capability_records: Vec<CapabilitySemanticRecord>,
    pub objects: Vec<ObjectSnapshot>,
    pub links: Vec<LinkSnapshot>,
    pub scopes: Vec<ScopeSnapshot>,
    pub global_version: Version,
    pub object_id_counter: u64,
    pub grant_sequence: u64,
    /// Identity of the last successfully applied Delta (constitution §3.4 / §4).
    /// Genesis / no applied delta = ZERO_HASH.
    pub last_applied_delta_hash: [u8; 32],
}

/// Object 的稳定语义快照。不绑定 ObjectRecord 内部布局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSnapshot {
    pub id: ObjectId,
    pub object_type: ObjectType,
    pub lifecycle_state: ObjectState,
    pub metadata: Vec<u8>,
    pub payload: Vec<u8>,
}

/// Link 的稳定语义快照。结构体形式支持未来扩展字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSnapshot {
    pub from: ObjectId,
    pub to: ObjectId,
    pub link_type: LinkType,
}

/// Scope 的稳定语义快照。owner 使用 ObjectId，不暴露 ModuleId。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSnapshot {
    pub scope_id: ScopeId,
    pub members: Vec<StateId>,
    pub owner: ObjectId,
    pub struct_version: Version,
}
