Veritas 运行时数据模型标准

文档版本：1.0
配套设计文档：Veritas V0.6
状态：定版

---

零、前言

本文档是 Veritas V0.6 设计文档的配套标准。V0.6 定义了“机器应该做什么”，本文档定义“机器在内存里到底长什么样”。

阅读本文档后，Rust 程序员应能直接建立 types.rs，定义所有核心数据结构，并开始编写引擎逻辑。

---

一、基础类型定义

1.1 全局版本号

```
类型：u64
Rust表示：AtomicU64
语义：严格单调递增的逻辑时钟

规则：
- 机器启动时初始化为 0
- 仅当事务成功通过提交临界区、WriteSet 正式写入全局状态后，才执行一次 fetch_add(1)
- 事务 BEGIN 时记录的快照版本、ReadSet 中的版本、每个 State 的当前版本，全部使用此类型
- 不是时间戳，不是随机数，只是单调计数器
```

1.2 核心标识符

所有标识符统一为 u64，由机器内部生成，不对宿主语言暴露构造器。

类型 Rust表示 生成方式 说明
StateId u64 确定性哈希（见1.3节） 状态的全局唯一标识
ScopeId u64 确定性哈希 不变量作用域的全局唯一标识
CapabilityId u64 机器随机生成（不可伪造） 能力凭证的句柄
TxId u64 原子递增计数器 事务的全局唯一标识
ModuleId u64 确定性哈希 模块的全局唯一标识

1.3 StateId 的确定性哈希规则

StateId 必须稳定：同一源码路径在不同编译中生成相同 StateId，否则 WAL 无法跨版本恢复。

算法： 固定种子的 SipHash-2-4（或 xxHash64 固定种子）

输入字符串格式： "ModuleName::EntityName::FieldName"

示例：

```
"AccountModule::Account_A::Balance" → 0x8A3F_21E7_B109_4C56
```

规则：

· 哈希种子在 Veritas 规范中固定，永不改变
· 禁止运行期随机生成
· LOAD 时若检测到 StateId 碰撞（两个不同路径哈希到同一值），拒绝 LOAD 并报错
· 建议底层存储同时保存完整路径字符串用于碰撞检测和错误诊断

1.4 CapabilityId 的不可伪造规则

· CapabilityId 的唯一构造路径是 GRANT 原语
· 构造使用机器内部 CSPRNG（密码学安全伪随机数生成器）
· 不对宿主语言暴露任何 CapabilityId::new() 或等效构造器
· 程序员只能持有 CapabilityId 值，无法自行创建

---

二、核心数据结构

2.1 全局状态存储

```
StateStore: HashMap<StateId, StateEntry>

StateEntry {
    value: Vec<u8>,          // 状态值（字节序列，模块自行序列化）
    version: u64,            // 该状态当前的全局版本号
    owner: ModuleId,         // 所属模块
    path: String,            // 完整源码路径（用于碰撞检测和调试）
}
```

2.2 SCOPE 注册表

```
ScopeRegistry: HashMap<ScopeId, ScopeEntry>

ScopeEntry {
    members: HashSet<StateId>,   // 当前属于该Scope的所有StateId
    struct_version: u64,         // Scope的结构版本号（成员变更时递增）
    owner: ModuleId,             // 所属模块
}
```

结构版本号规则：

· 初始值为 0
· 任何 BIND_TO_SCOPE（新增成员）或 UNBIND_FROM_SCOPE（移除成员）成功提交后递增
· 仅当成员集合结构变化时递增，成员的状态值变化不影响结构版本号

2.3 能力委托图

```
CapabilityGraph {
    grants: HashMap<CapabilityId, CapabilityInfo>,
    delegations: Vec<DelegationEdge>,     // 有向边：授权者→被授权者
}

CapabilityInfo {
    capability_type: String,              // 能力类型标识（如"转账"、"读取"）
    current_holder: ModuleId,             // 当前持有者
    granted_by: ModuleId,                 // 原始授予者
}

DelegationEdge {
    from: ModuleId,                       // 授权者
    to: ModuleId,                         // 被授权者
    capability_id: CapabilityId,
    cascade_on_revoke: bool,              // 撤销时是否级联
}
```

级联撤销规则：

· REVOKE 时，从委托图中找到对应边
· 若 cascade_on_revoke == true，从被授权者出发递归查找所有通过该边可达的间接委托，全部失效
· 若 cascade_on_revoke == false，仅移除直接边，间接委托保留

2.4 事务上下文

```
TransactionContext {
    tx_id: TxId,
    snapshot_version: u64,                // BEGIN时记录的全局版本号
    read_set: ReadSet,
    write_set: WriteSet,
    effect_queue: Vec<EffectRecord>,
    effect_sequence: u64,                 // 事务内EFFECT序列号计数器
    savepoints: Vec<Savepoint>,
    aborted: bool,
}

ReadSet {
    states: HashMap<StateId, u64>,        // StateId → 读取时的版本号
    scopes: HashMap<ScopeId, u64>,        // ScopeId → 枚举时的结构版本号
}

WriteSet {
    state_changes: HashMap<StateId, Vec<u8>>,      // 暂存的状态修改
    scope_changes: Vec<ScopeChange>,                // 暂存的Scope结构变更
}

ScopeChange {
    scope_id: ScopeId,
    change_type: ScopeChangeType,
    state_id: StateId,
}

ScopeChangeType: Enum {
    Bind,        // BIND_TO_SCOPE：新增成员
    Unbind,      // UNBIND_FROM_SCOPE：移除成员
}

EffectRecord {
    idempotency_key: String,              // 格式："{TxId}-{Sequence}"
    operation: Box<dyn FnOnce()>,         // 副作用闭包
}

Savepoint {
    name: String,
    write_set_snapshot: HashMap<StateId, Vec<u8>>,   // 创建时的写入集快照
    effect_queue_length: usize,                       // 创建时副作用队列长度
}
```

2.5 预写日志（WAL）

```
WAL: 只追加的持久化日志文件

WALEntry: Enum {
    Commit {
        tx_id: TxId,
        state_changes: Vec<(StateId, Vec<u8>)>,      // 该事务的所有状态修改
        scope_changes: Vec<ScopeChange>,              // 该事务的Scope结构变更
        capability_changes: Vec<CapabilityChange>,    // 该事务的能力变更
    },
    EffectAck {
        tx_id: TxId,
        idempotency_key: String,                      // 副作用已执行的确认
    },
    Checkpoint {
        global_version: u64,                          // 检查点时的全局版本号
    },
}

CapabilityChange: Enum {
    Grant { cap_id: CapabilityId, cap_type: String, from: ModuleId, to: ModuleId },
    Delegate { cap_id: CapabilityId, from: ModuleId, to: ModuleId, cascade: bool },
    Revoke { cap_id: CapabilityId, from: ModuleId },
}
```

WAL 写入规则：

· 仅在提交临界区内写入
· 写入内容仅为最终有效的修改（已剔除 ROLLBACK_TO 撤销部分）
· 每次 COMMIT 对应一条 WAL Entry
· EffectAck 在副作用执行成功后异步写入

---

三、机器全局状态

```
VeritasEngine {
    global_version: AtomicU64,                     // 全局版本号
    state_store: RwLock<HashMap<StateId, StateEntry>>,
    scope_registry: RwLock<HashMap<ScopeId, ScopeEntry>>,
    capability_graph: RwLock<CapabilityGraph>,
    active_transactions: HashMap<TxId, TransactionContext>,
    wal: WriteAheadLog,
    commit_lock: Mutex<()>,                        // 全局提交临界区锁
    tx_id_counter: AtomicU64,                      // 事务ID计数器
    modules: HashMap<ModuleId, ModuleDefinition>,   // 已加载模块
}
```

3.1 模块定义

```
ModuleDefinition {
    module_id: ModuleId,
    contracts: Vec<ContractDefinition>,
    operations: HashMap<String, OperationDefinition>,
    reentrant_allowed: bool,
}

ContractDefinition {
    contract_type: ContractType,
    expression: Box<dyn Fn(&TransactionContext) -> bool>,  // 纯函数
}

ContractType: Enum {
    Require,
    Ensure,
    Invariant,
}

OperationDefinition {
    name: String,
    body: Box<dyn Fn(&mut TransactionContext)>,
}
```

3.2 契约函数的纯函数约束

· 契约函数（REQUIRE、ENSURE、INVARIANT）必须是纯函数
· 只允许读取当前事务上下文中的状态
· 禁止修改状态
· 禁止执行 EFFECT
· 禁止访问外部 IO（网络、文件、标准输出等）
· 违反以上规则的契约函数，其行为不在机器保护范围内
· 原型阶段由程序员自行保证，未来可迁移至 WASM 沙箱强制执行

---

四、关键流程的数据操作

4.1 BEGIN

```
fn begin() -> TxId {
    let tx_id = engine.tx_id_counter.fetch_add(1);
    let snapshot = engine.global_version.load(Ordering::Acquire);
    
    let ctx = TransactionContext {
        tx_id,
        snapshot_version: snapshot,
        read_set: ReadSet::empty(),
        write_set: WriteSet::empty(),
        effect_queue: Vec::new(),
        effect_sequence: 0,
        savepoints: Vec::new(),
        aborted: false,
    };
    
    engine.active_transactions.insert(tx_id, ctx);
    tx_id
}
```

4.2 状态读取（自动追踪）

```
fn read_state(ctx: &mut TransactionContext, state_id: StateId) -> Vec<u8> {
    let store = engine.state_store.read();
    let entry = store.get(&state_id).expect("State not found");
    
    // 自动记录到ReadSet
    ctx.read_set.states.insert(state_id, entry.version);
    
    entry.value.clone()
}
```

4.3 状态写入（暂存）

```
fn write_state(ctx: &mut TransactionContext, state_id: StateId, value: Vec<u8>) {
    ctx.write_set.state_changes.insert(state_id, value);
}
```

4.4 ENUM_SCOPE

```
fn enum_scope(ctx: &mut TransactionContext, scope_id: ScopeId) -> Vec<StateId> {
    let registry = engine.scope_registry.read();
    let entry = registry.get(&scope_id).expect("Scope not found");
    
    // 记录Scope的结构版本号到ReadSet
    ctx.read_set.scopes.insert(scope_id, entry.struct_version);
    
    entry.members.iter().cloned().collect()
}
```

4.5 BIND_TO_SCOPE

```
fn bind_to_scope(ctx: &mut TransactionContext, scope_id: ScopeId, state_id: StateId) {
    ctx.write_set.scope_changes.push(ScopeChange {
        scope_id,
        change_type: ScopeChangeType::Bind,
        state_id,
    });
}
```

4.6 COMMIT

```
fn commit(ctx: &mut TransactionContext) -> Result<(), AbortReason> {
    // 【进入全局提交临界区】
    let _lock = engine.commit_lock.lock();
    
    // 步骤1：冲突检测
    // 基础检测
    for (state_id, read_version) in &ctx.read_set.states {
        let store = engine.state_store.read();
        if let Some(entry) = store.get(state_id) {
            if entry.version > *read_version {
                return Err(AbortReason::WriteConflict);
            }
        }
    }
    
    // 扩展检测（幻读）
    for (scope_id, read_struct_version) in &ctx.read_set.scopes {
        let registry = engine.scope_registry.read();
        if let Some(entry) = registry.get(scope_id) {
            if entry.struct_version > *read_struct_version {
                return Err(AbortReason::PhantomConflict);
            }
        }
    }
    
    // 步骤2：写入WAL
    let wal_entry = WALEntry::Commit {
        tx_id: ctx.tx_id,
        state_changes: ctx.write_set.state_changes.iter()
            .map(|(id, val)| (*id, val.clone())).collect(),
        scope_changes: ctx.write_set.scope_changes.clone(),
        capability_changes: Vec::new(), // 按实际能力变更填充
    };
    engine.wal.append_and_flush(wal_entry);
    
    // 步骤3：状态固化
    let mut store = engine.state_store.write();
    for (state_id, value) in &ctx.write_set.state_changes {
        if let Some(entry) = store.get_mut(state_id) {
            entry.value = value.clone();
            entry.version = engine.global_version.load(Ordering::Relaxed) + 1;
        }
    }
    
    // Scope结构变更
    let mut registry = engine.scope_registry.write();
    for change in &ctx.write_set.scope_changes {
        if let Some(entry) = registry.get_mut(&change.scope_id) {
            match change.change_type {
                ScopeChangeType::Bind => {
                    entry.members.insert(change.state_id);
                    entry.struct_version += 1;
                }
                ScopeChangeType::Unbind => {
                    entry.members.remove(&change.state_id);
                    entry.struct_version += 1;
                }
            }
        }
    }
    
    // 递增全局版本号
    engine.global_version.fetch_add(1, Ordering::Release);
    
    // 【退出临界区】
    drop(_lock);
    
    // 执行副作用
    for effect in &ctx.effect_queue {
        (effect.operation)();
        let ack = WALEntry::EffectAck {
            tx_id: ctx.tx_id,
            idempotency_key: effect.idempotency_key.clone(),
        };
        engine.wal.append_and_flush(ack);
    }
    
    engine.active_transactions.remove(&ctx.tx_id);
    Ok(())
}
```

4.7 ABORT

```
fn abort(ctx: &mut TransactionContext, reason: AbortReason) {
    ctx.aborted = true;
    // 写入集和副作用队列随上下文一起丢弃
    engine.active_transactions.remove(&ctx.tx_id);
    // 按权限脱敏返回原因（调用方处理）
}
```

4.8 崩溃恢复

```
fn recover() {
    let entries = engine.wal.scan_from_last_checkpoint();
    
    for entry in entries {
        match entry {
            WALEntry::Commit { tx_id, state_changes, scope_changes, capability_changes } => {
                // 重放状态修改
                let mut store = engine.state_store.write();
                for (state_id, value) in state_changes {
                    if let Some(entry) = store.get_mut(&state_id) {
                        entry.value = value;
                        entry.version += 1;
                    }
                }
                
                // 重放Scope变更
                let mut registry = engine.scope_registry.write();
                for change in scope_changes {
                    if let Some(entry) = registry.get_mut(&change.scope_id) {
                        match change.change_type {
                            ScopeChangeType::Bind => {
                                entry.members.insert(change.state_id);
                                entry.struct_version += 1;
                            }
                            ScopeChangeType::Unbind => {
                                entry.members.remove(&change.state_id);
                                entry.struct_version += 1;
                            }
                        }
                    }
                }
                
                // 重建能力图
                // ... 根据 capability_changes 重建
                
                // 更新全局版本号
                engine.global_version.fetch_add(1, Ordering::Release);
            }
            WALEntry::EffectAck { .. } => {
                // 已确认，跳过
            }
            WALEntry::Checkpoint { global_version } => {
                engine.global_version.store(global_version, Ordering::Release);
            }
        }
    }
    
    // 重试未确认的副作用
    // 扫描COMMIT记录中未找到对应EffectAck的副作用，放入重试队列
}
```

---

五、序列化格式（WAL 持久化）

WAL 条目在磁盘上的二进制格式：

```
WAL Entry Header:
[4 bytes] CRC32 checksum
[1 byte ] Entry type (0x01=Commit, 0x02=EffectAck, 0x03=Checkpoint)
[8 bytes] Entry length (不包括Header)

Commit Entry Body:
[8 bytes] TxId
[4 bytes] State change count
For each state change:
  [8 bytes] StateId
  [4 bytes] Value length
  [N bytes] Value bytes
[4 bytes] Scope change count
For each scope change:
  [8 bytes] ScopeId
  [1 byte ] Change type (0x00=Bind, 0x01=Unbind)
  [8 bytes] StateId

EffectAck Entry Body:
[8 bytes] TxId
[4 bytes] Idempotency key length
[N bytes] Idempotency key bytes

Checkpoint Entry Body:
[8 bytes] Global version
```

---

六、并发安全说明

数据结构 同步机制 说明
global_version AtomicU64 无锁读取，提交临界区内递增
state_store RwLock 多读单写。读取时获取读锁，提交时获取写锁
scope_registry RwLock 同上
capability_graph RwLock 同上
active_transactions 单线程访问或外部同步 事务上下文仅由其所有者线程访问
commit_lock Mutex 保证提交临界区互斥
tx_id_counter AtomicU64 无锁递增
wal 内部同步 WAL 实现自行处理并发写入

---

本文档与 Veritas V0.6 设计文档配套使用。设计文档定义“为什么”，本文档定义“是什么”。两者共同构成 Rust 原型开发的完整蓝图。
