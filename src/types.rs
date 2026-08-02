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

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone)]
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
    pub aborted: bool,
    pub capability_enforced: bool,
    /// 当前执行上下文所属的Object。一切Read/Write在没有显式CALL切换的情况下，
    /// 隐式作用于这个Object的Memory Space——这是Memory宪法(memory.md)第4节
    /// "地址 = (ObjectId, StateId)"在执行层的落地方式：地址的ObjectId分量
    /// 来自当前上下文，不需要每条指令自己携带。
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
            pending_links: Vec::new(),
            pending_unlinks: Vec::new(),
            pending_freezes: Vec::new(),
            pending_deaths: Vec::new(),
            aborted: false,
            capability_enforced: true,
            current_object: 0,
        }
    }

    pub fn enforce_capability(&mut self) {
        self.capability_enforced = true;
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
}
