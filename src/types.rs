// Veritas Kernel V0.1 - 核心类型定义
// 配套文档：docs/Veritas_V0.6_设计文档.txt
//
// 已知局限（Phase 1）：
// - 快照隔离采用fail-fast OCC近似，不维护历史版本链
// - 读超前数据时直接ABORT，而非返回BEGIN时的快照值
// - 真MVCC版本链留待后续性能阶段实现

use std::collections::HashMap;

/// 全局单调递增版本号
pub type Version = u64;

/// 状态标识符（确定性哈希生成）
pub type StateId = u64;

/// 作用域标识符
pub type ScopeId = u64;

/// 事务标识符
pub type TxId = u64;

/// 模块标识符
pub type ModuleId = u64;

/// 状态条目
#[derive(Debug, Clone)]
pub struct StateEntry {
    /// 状态值（字节序列）
    pub value: Vec<u8>,
    /// 该状态当前的版本号
    pub version: Version,
}

/// 读取集：记录事务读取了哪些状态及其版本号
/// 注：盲写时也会隐式将当前版本补入read_set，以覆盖写-写冲突
#[derive(Debug, Clone, Default)]
pub struct ReadSet {
    /// StateId → 读取时的版本号
    pub states: HashMap<StateId, Version>,
}

/// 写入集：事务暂存的状态修改
#[derive(Debug, Clone, Default)]
pub struct WriteSet {
    /// StateId → 新值
    pub state_changes: HashMap<StateId, Vec<u8>>,
}

/// 事务上下文
#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub tx_id: TxId,
    /// BEGIN时记录的全局版本号快照
    pub snapshot_version: Version,
    pub read_set: ReadSet,
    pub write_set: WriteSet,
    pub aborted: bool,
}

/// 中止原因
#[derive(Debug, Clone, PartialEq)]
pub enum AbortReason {
    /// 写冲突：读过的状态被其他事务修改了
    WriteConflict,
    /// 读取到了超前版本（当前fail-fast策略下的快照隔离违规）
    ReadFutureVersion,
    /// 事务已被中止
    AlreadyAborted,
    /// 状态未找到
    StateNotFound,
}

impl std::fmt::Display for AbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbortReason::WriteConflict => write!(f, "Write conflict detected"),
            AbortReason::ReadFutureVersion => write!(f, "Read future version - snapshot too old"),
            AbortReason::AlreadyAborted => write!(f, "Transaction already aborted"),
            AbortReason::StateNotFound => write!(f, "State not found"),
        }
    }
}

/// 机器错误类型
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

/// 确定性哈希函数（FNV-1a 64位，固定种子，永不改变）
/// 保证同一源码路径在不同编译中生成相同StateId
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
