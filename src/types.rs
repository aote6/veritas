// Veritas Kernel V0.2 - 核心类型定义

use std::collections::HashMap;

pub type Version = u64;
pub type StateId = u64;
pub type ScopeId = u64;
pub type TxId = u64;
pub type ModuleId = u64;

#[derive(Debug, Clone)]
pub struct StateEntry {
    pub value: Vec<u8>,
    pub version: Version,
}

#[derive(Debug, Clone, Default)]
pub struct ReadSet {
    pub states: HashMap<StateId, Version>,
}

#[derive(Debug, Clone, Default)]
pub struct WriteSet {
    /// 按写入顺序存储，支持回滚到任意 savepoint
    pub changes: Vec<(StateId, Vec<u8>)>,
}

impl WriteSet {
    /// 插入一条写入记录
    pub fn push(&mut self, state_id: StateId, value: Vec<u8>) {
        self.changes.push((state_id, value));
    }

    /// 获取某个 state 的最新值
    pub fn get_latest(&self, state_id: StateId) -> Option<&Vec<u8>> {
        self.changes
            .iter()
            .rev()
            .find(|(id, _)| *id == state_id)
            .map(|(_, val)| val)
    }

    /// 检查某个 state 是否在写入集中
    pub fn contains_key(&self, state_id: StateId) -> bool {
        self.changes.iter().any(|(id, _)| *id == state_id)
    }

    /// 获取写入集的长度（用于 savepoint）
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// 截断到指定长度（用于 rollback）
    pub fn truncate(&mut self, len: usize) {
        self.changes.truncate(len);
    }

    /// 获取所有 state_id 的集合（用于迭代）
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

    /// 获取所有写入记录的迭代器
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
}

#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub tx_id: TxId,
    pub snapshot_version: Version,
    pub read_set: ReadSet,
    pub write_set: WriteSet,
    pub effect_queue: EffectQueue,
    pub savepoints: Vec<Savepoint>,
    pub aborted: bool,
}

impl TransactionContext {
    pub fn new(tx_id: TxId, snapshot_version: Version) -> Self {
        TransactionContext {
            tx_id,
            snapshot_version,
            read_set: ReadSet::default(),
            write_set: WriteSet::default(),
            effect_queue: EffectQueue::default(),
            savepoints: Vec::new(),
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
        self.write_set.changes.clear();
        self.effect_queue = EffectQueue::default();
        self.savepoints.clear();
        self.aborted = false;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AbortReason {
    WriteConflict,
    ReadFutureVersion,
    AlreadyAborted,
    StateNotFound,
}

impl std::fmt::Display for AbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbortReason::WriteConflict => write!(f, "Write conflict detected"),
            AbortReason::ReadFutureVersion => write!(f, "Read future version"),
            AbortReason::AlreadyAborted => write!(f, "Transaction already aborted"),
            AbortReason::StateNotFound => write!(f, "State not found"),
        }
    }
}

#[derive(Debug)]
pub enum VeritasError {
    Abort(AbortReason),
    EngineError(String),
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
