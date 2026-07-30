// Veritas Kernel V0.2 - 核心类型定义

use std::collections::HashMap;

pub type Version = u64;
pub type StateId = u64;
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
    Dead,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    CapabilityDelegation = 0,
    ContractDependency = 1,
    EffectPropagation = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEdge {
    pub from: ObjectId,
    pub to: ObjectId,
    pub relation: RelationKind,
}


#[derive(Debug, Clone, Default)]
pub struct ReadSet {
    pub states: HashMap<StateId, Version>,
    pub scopes: HashMap<ScopeId, Version>,
}

#[derive(Debug, Clone, Default)]
pub struct WriteSet {
    /// 按写入顺序存储，支持回滚到任意 savepoint
    pub changes: Vec<(StateId, Vec<u8>)>,
}

impl WriteSet {
    pub fn push(&mut self, state_id: StateId, value: Vec<u8>) {
        self.changes.push((state_id, value));
    }

    pub fn get_latest(&self, state_id: StateId) -> Option<&Vec<u8>> {
        self.changes
            .iter()
            .rev()
            .find(|(id, _)| *id == state_id)
            .map(|(_, val)| val)
    }

    pub fn contains_key(&self, state_id: StateId) -> bool {
        self.changes.iter().any(|(id, _)| *id == state_id)
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

    pub fn keys(&self) -> Vec<StateId> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for (id, _) in &self.changes {
            if !seen.contains(id) {
                seen.insert(*id);
                result.push(*id);
            }
        }
        result
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (StateId, Vec<u8>)> {
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
    pub capability_id: Option<u64>,
    pub program_hash: Option<u64>,
    pub tx_id: TxId,
    pub snapshot_version: Version,
    pub read_set: ReadSet,
    pub write_set: WriteSet,
    pub scope_write_set: Vec<ScopeChange>,
    pub effect_queue: EffectQueue,
    pub savepoints: Vec<Savepoint>,
    pub pending_links: Vec<LinkEdge>,
    pub pending_deaths: Vec<ObjectId>,
    pub pending_objects: Vec<ObjectId>,
    pub aborted: bool,
}

impl TransactionContext {
    pub fn new(tx_id: TxId, snapshot_version: Version) -> Self {
        TransactionContext {
            capability_id: None,
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
            pending_deaths: Vec::new(),
            aborted: false,
        }
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrapFrame {
    pub pc: usize,
    pub reason: TrapReason,
    pub cycles: u64,
}
