Veritas 运行时数据模型标准

文档版本：1.1
配套设计文档：Veritas V0.6
状态：已按代码实际实现修正

---
零、前言

本文档是 Veritas V0.6 设计文档的配套标准。V0.6 定义了"机器应该做什么"，本文档定义"机器在内存里到底长什么样"。

阅读本文档后，Rust 程序员应能直接建立 types.rs，定义所有核心数据结构，并开始编写引擎逻辑。

---

一、基础类型定义

1.1 全局版本号

类型：u64
Rust表示：AtomicU64
语义：严格单调递增的逻辑时钟

规则：
- 机器启动时初始化为 0
- 仅当事务成功通过提交临界区、WriteSet 正式写入全局状态后，才执行一次 fetch_add(1)
- 事务 BEGIN 时记录的快照版本、ReadSet 中的版本、每个 State 的当前版本，全部使用此类型
- 不是时间戳，不是随机数，只是单调计数器

1.2 核心标识符

所有标识符统一为 u64，由机器内部生成，不对宿主语言暴露构造器。

类型       Rust表示  生成方式                    说明
StateId    u64       确定性哈希（见1.3节）       状态的全局唯一标识
ScopeId    u64       确定性哈希                  不变量作用域的全局唯一标识
CapabilityId u64     确定性哈希（见1.4节修正）   能力凭证的句柄
TxId       u64       原子递增计数器              事务的全局唯一标识
ModuleId   u64       确定性哈希                  模块的全局唯一标识

1.3 StateId 的确定性哈希规则

StateId 必须稳定：同一源码路径在不同编译中生成相同 StateId，否则 WAL 无法跨版本恢复。

算法：FNV-1a（原型阶段，后续可切换 SipHash/xxHash64）

输入字符串格式： "ModuleName::EntityName::FieldName"

规则：
· 禁止运行期随机生成
· 同一路径字符串始终生成同一 StateId
· ScopeId 使用相同算法，输入前缀为 __scope__:
· CapabilityId 使用相同算法，输入格式见 1.4 节

1.4 CapabilityId 的确定性哈希规则（V1.1 修正）

原 V1.0 标准要求 CapabilityId 由机器随机生成（CSPRNG），理由是不可伪造。
但 Veritas 引擎的核心哲学是确定性可重放——WAL replay 两次必须产生完全一样的状态。
随机生成的 CapabilityId 会导致两次 replay 产生不同 ID，与 State/Scope 的确定性设计原则直接冲突。

修正：CapabilityId 沿用 StateId/ScopeId 的确定性哈希模式。

输入字符串格式： "__cap__:{grantor}:{grantee}:{resource}:{grant_sequence}"

其中 grant_sequence 是 Capability 子系统自己的单调计数器（不借用 global_version 或 tx_id），
每次 GRANT 时递增。这样：
· 同一事务内 grant→rollback→再 grant 不会哈希碰撞（grant_sequence 已前进）
· 崩溃恢复后从 WAL 取 max_sequence + 1 续接，与 tx_id_counter 恢复同一套逻辑

不可伪造性的保证：CapabilityId 的构造器不对宿主语言暴露，仅由 GRANT 原语生成。
程序员只能持有 CapabilityId 值，无法自行创建——伪造的关键是拿不到构造权，不是猜不出数值。
随机数在这里是抄了一个不适合本引擎设计哲学的教条。

---

二、状态存储层

2.1 StateEntry

struct StateEntry {
    value: Vec<u8>,        // 状态值（不透明字节序列）
    version: Version,      // 该状态的当前版本号
}

注意：
· 当前版本未实现 owner 和 path 字段（模块系统未完成）
· 所有状态存储在全局 HashMap<StateId, StateEntry> 中
· 修改只在事务 COMMIT 时发生

2.2 StateStore

struct StateStore {
    states: RwLock<HashMap<StateId, StateEntry>>,
}

关键操作：
- read(state_id) → Option<StateEntry>：获取当前快照
- insert(state_id, entry)：仅在 COMMIT 时调用
- from_map(map)：从 WAL 恢复时使用

---

三、Scope 结构管理层

3.1 ScopeEntry

struct ScopeEntry {
    members: Vec<StateId>,      // 当前成员列表
    struct_version: Version,    // 结构版本号（独立于成员状态版本）
    owner: ModuleId,            // 所属模块
}

关键规则：
· struct_version 仅在 bind/unbind 时递增
· 成员自身的 value version 变化不影响 struct_version
· 这就是幻读防御的基础

3.2 ScopeRegistry

struct ScopeRegistry {
    scopes: RwLock<HashMap<ScopeId, ScopeEntry>>,
}

关键操作：
- declare(scope_id, owner)：幂等声明 Scope
- snapshot(scope_id) → Option<(Vec<StateId>, Version)>：获取成员列表 + 结构版本
- struct_version(scope_id) → Option<Version>：仅获取结构版本
- apply_bind / apply_unbind：仅在 COMMIT 时调用
- from_map(map)：从 WAL 恢复时使用

3.3 ReadSet 中的 Scope 追踪

ReadSet 现在包含两个字段：
- states: HashMap<StateId, Version>     // 读过的状态 + 读取时版本
- scopes: HashMap<ScopeId, Version>     // 枚举过的 Scope + 当时结构版本

当调用 ENUM_SCOPE 时，自动将当前 Scope 的 struct_version 记入 read_set.scopes。
COMMIT 时 detect_scope_conflict() 比对 struct_version 是否变化——这就是幻读检测。

---

四、事务层

4.1 TransactionContext

struct TransactionContext {
    tx_id: TxId,
    snapshot_version: Version,
    read_set: ReadSet,
    write_set: WriteSet,
    scope_write_set: Vec<ScopeChange>,
    effect_queue: EffectQueue,
    savepoints: Vec<Savepoint>,
    aborted: bool,
}

4.2 WriteSet

struct WriteSet {
    changes: Vec<(StateId, Vec<u8>)>,  // 按写入顺序存储，支持回滚到任意 savepoint
}

关键操作：
- push(state_id, value)：追加写入
- get_latest(state_id) → Option<&Vec<u8>>：读自己的写
- truncate(len)：回滚时截断到 savepoint 长度

4.3 EffectQueue

struct EffectQueue {
    effects: Vec<PendingEffect>,
}

struct PendingEffect {
    idempotency_key: String,   // 系统强制生成 {tx_id}-{seq}
    payload: Vec<u8>,
}

4.4 ScopeChange

struct ScopeChange {
    scope_id: ScopeId,
    state_id: StateId,
    change_type: ScopeChangeType,  // Bind 或 Unbind
}

Scope 结构变更先写入 ctx.scope_write_set，COMMIT 时才真正落地到 ScopeRegistry。
这保证了 savepoint/rollback 可以正确撤销 scope 变更。

4.5 Savepoint

struct Savepoint {
    name: String,
    write_set_len: usize,
    effect_queue_len: usize,
    scope_write_set_len: usize,   // P0 新增：scope 变更也可以回滚
}

---

五、WAL（预写日志）层

5.1 WalEntry 三态枚举（P1 实现，V1.1 修正）

enum WalEntry {
    Commit {
        tx_id: TxId,
        version: Version,
        writes: Vec<(StateId, Vec<u8>)>,
        scope_changes: Vec<WalScopeChange>,
        effects: Vec<WalEffect>,       // V1.1 修正：原 V1.0 未包含此字段
    },
    EffectAck {
        tx_id: TxId,
        idempotency_key: String,
    },
    Checkpoint {
        version: Version,
    },
}

注意：
· effects 字段是 V1.1 修正新增的。原 V1.0 标准未定义此字段，但设计文档 4.8 节
  崩溃恢复流程要求"扫描 COMMIT 记录中未找到 EffectAck 的副作用"——若 Commit
  记录不存 effect 内容，此流程无法实现。此处补上 effects 字段以消解文档矛盾。
· 当前使用文本格式（非二进制 CRC32），原型阶段排障便利优先。
  CRC32 二进制格式留作独立 P1.5。

5.2 文本序列化格式

COMMIT TX={tx_id} VERSION={version} [WRITE {state_id} {hex_value}]... [SCOPEBIND {scope_id} {state_id}]... [SCOPEUNBIND {scope_id} {state_id}]... [EFFECT {key} {hex_payload}]... END

EFFECTACK TX={tx_id} KEY={idempotency_key} END

CHECKPOINT VERSION={version} END

5.3 RecoveryManager

崩溃恢复完整流程：
1. recover() 扫描 WAL 文件，返回 Vec<WalEntry> 和 max_version
2. apply_records() 重放所有记录，返回：
   - state_map: HashMap<StateId, StateEntry>
   - scope_map: HashMap<ScopeId, ScopeEntry>
   - pending_effects: Vec<PendingRecoveryEffect>（已提交但未确认 EffectAck 的副作用）
   - max_tx_id: TxId（用于 tx_id_counter 续接）
3. 引擎启动时：
   - 从 state_map 和 scope_map 重建状态
   - 重试 pending_effects 中的副作用并补写 EffectAck
   - tx_id_counter 从 max_tx_id + 1 开始

5.4 与标准格式的偏差（明确记录）

1. 文本格式 vs 二进制 CRC32：原型阶段 cat wal.log 可读排障 > 校验效率。
   已有 11 个 WAL 测试覆盖 round-trip。CRC32 留作 P1.5。
2. WalEntry::Commit 补 effects 字段：消解设计文档 4.8 节与标准 V1.0 的矛盾。

---

六、能力系统层（P2 Step 1 已实现纯数据层）

6.1 CapabilityId（已修正，见 1.4 节）

6.2 CapabilityGraph（纯数据层，尚未接入事务）

struct CapabilityGraph {
    grants: HashMap<CapabilityId, CapabilityInfo>,
    holders: HashMap<(CapabilityId, ModuleId), HolderRecord>,
    children: HashMap<(CapabilityId, ModuleId), HashSet<ModuleId>>,
    edges: Vec<DelegationEdge>,
    grant_sequence: u64,          // 独立单调计数器
}

struct CapabilityInfo {
    capability_type: String,
    granted_by: ModuleId,
    root_holder: ModuleId,
    resource: ResourceId,         // 当前统一用 StateId
}

struct HolderRecord {
    active: bool,                 // revoke 只翻转此标志，不物理删除
    parent: Option<ModuleId>,     // None = 根节点
}

struct DelegationEdge {
    from: ModuleId,
    to: ModuleId,
    capability_id: CapabilityId,
    cascade_on_revoke: bool,
}

6.3 委托图约束（森林结构）

CapabilityGraph 是森林（每个 capability_id 对应一棵树），不是一般 DAG。

约束：
1. 每个 (capability_id, holder) 组合只会被插入一次
2. 插入后的 HolderRecord 永不物理删除（revoke 只翻转 active 标志）

推论：不可能成环——因为若 to 是 from 的祖先，to 必然已被插入过，
插入时的"已在树中"检查（AlreadyInTree）会直接拒绝，无需额外环检测算法。

6.4 revoke 语义

· 级联(cascade=true)：holder 自己 + 整个下游子树的 active 全部置 false
· 非级联(cascade=false)：只把 holder 自己的 active 置 false，下游子孙的 active 不变
· holds() 只看 (cap_id, holder) 自己的 active 标志，不做树遍历
· parent/children 索引仅用于"级联时要往下影响谁"

6.5 当前状态

P2 Step 1 已完成纯数据层（src/capability.rs），12 个测试全部通过。
尚未接入事务、WAL、engine.rs。这部分留作 P2 Step 2-6。

---

七、版本修正记录

版本  变更
1.0   初始版本，配套 V0.6 设计文档
1.1   修正：CapabilityId 从随机生成改为确定性哈希（1.4 节）；
      修正：WalEntry::Commit 补 effects 字段（5.1 节）；
      补充：ScopeRegistry、WriteSet、Savepoint、CapabilityGraph 实际数据结构；
      补充：WAL 文本格式说明和偏差记录。
