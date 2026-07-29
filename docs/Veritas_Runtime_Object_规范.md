# Veritas Runtime Object 规范

文档版本：1.0（物理确权·定版）
配套文档：Veritas V0.6 设计文档、运行时数据模型标准 V1.0
状态：定版，准备进入原型开发

---

## 零、前言与设计哲学

在 Veritas V0.6 之前，系统只有规则（事务、WAL、Scope、Capability），没有实体。状态游离在全局 HashMap 中，能力挂在抽象节点上，导致 OBJECT_BIRTH 容易沦为对底层存储的 CRUD 凑代码操作。

Veritas 定义软件世界的物理定律。
CPU 的物理基本粒子是内存单元与寄存器；
Veritas 抽象机器的基本粒子是 Runtime Object。

### 规则一：实体先于规则
事务不是实体，事务是改变世界的运动规律；Runtime Object 才是充盈在 Veritas 世界里的基本物质。所有 State、Capability、Contract、Effect 必须绑定且仅能绑定到 Runtime Object 上。

### 规则二：绝对物理隔离
没有任何 Runtime Object 可以跨越机器检查直接读取另一 Object 的内存字节。对象间的读写、授权与依赖，本质上是物理沙箱之间的受控穿孔（Controlled Hole-punching）。

---

## 一、Runtime Object 物理结构

在 Veritas 机器内存中，一个 Runtime Object 是一个独立的物理隔离拓扑节点，由以下 5 个不可分割的数据结构构成：

1. ObjectId
   类型：u64
   生成方式：固定种子的 SipHash-2-4 确定性哈希。
   输入格式：ModuleName::EntityName::InstancePath
   不可伪造性：不可由宿主语言（Rust）直接实例化，仅能通过内核 OBJECT_BIRTH 指令分配。

2. StateNamespace (状态根)
   Object 拥有的所有 StateId 必须强制带有 ObjectId 的前缀。
   物理保证：StateStore 查找时，若 StateId 前缀不匹配其所属 Object，内核读写机制强制拒绝（硬件级别的 MMU 内存越界保护）。

3. CapabilitySet
   记录该 Object 持有的 CapabilityId 映射表。
   维护双向索引：ObjectId 到 CapabilityId 以及 CapabilityId 到 Holder(ObjectId)。

4. ContractSet
   挂载专门针对该 Object 状态的不可变约束（Invariants）。
   对象的删除、修改必须触发其 ContractSet 内的纯函数校验。

5. LinkTopology (拓扑边)
   存储该 Object 与其他 Object 建立的可信信道边。
   每条边包含：TargetObjectId、RelationKind（授权/契约依赖/副作用传播）、EstablishTxId。

## 二、三条内核指令与物理保证

### 指令一：OBJECT_BIRTH (对象诞生)

OBJECT_BIRTH 绝不是 HashMap.insert。它是内存分配 + 隔离沙箱建立 + WAL 记录的原子物理动作。

#### 物理保证（Six Machine Guarantees）：

1. 隔离保证：自动分配物理 StateRoot 命名空间，未持有 Cap 的外部对象绝对不可见。

2. 快照隔离：在 COMMIT 前，处于暂存区。其他并发事务查询时视其为未诞生（Snapshot Isolation）。

3. 零痕迹 ABORT (Zero-Trace)：若事务 ABORT，内存数据结构直接丢弃，StateRoot 销毁，Cap 图回滚，WAL 中绝对不残留任何痕迹（Zero Trace）。

4. WAL 持久化：Birth 事件作为物理日志写入 WAL。恢复时顺序重放，恢复对象拓扑。

5. 能力隔离：诞生时刻初始化的初始 Capability 被原子隔离到其 CapabilitySet，无法被非法提取。

6. 唯一性保护：若 ObjectId 碰撞，直接抛出 ObjectAlreadyExists 并中断事务。

#### 执行流程：

BEGIN 事务
  OBJECT_BIRTH(path_str, initial_caps)
    步骤1. 计算 ObjectId = SipHash(path_str)
    步骤2. 检查全局 ObjectRegistry：若存在则 ABORT(ObjectAlreadyExists)
    步骤3. 在 WriteSet 暂存区中创建新 RuntimeObject 内存块
    步骤4. 初始化 StateRoot 命名空间、ContractSet 与 LinkTopology
    步骤5. 调用 Kernel::GRANT 生成初始 CapabilityId，填入 CapabilitySet
    步骤6. 将 WALEntry::ObjectBirth 追加至事务 Log
COMMIT 临界区 - 成功固化至全局 ObjectRegistry
ABORT - 彻底丢弃

### 指令二：OBJECT_LINK (关系建立)

OBJECT_LINK 用于在两个隔离的 Object 之间建立物理受控信道。

#### 关系类型：

1. CAPABILITY_DELEGATION：A 将自身某项能力委派给 B。
2. CONTRACT_DEPENDENCY：A 的局部不变量依赖 B 的状态读取。
3. EFFECT_PROPAGATION：A 产生的副作用会顺带触发 B 的响应。

#### 物理保证：

建立 LINK 必须同时满足：A 持有对 B 的穿孔能力 AND A 与 B 的契约无静态冲突。
任何基于 LINK 的穿孔访问，自动将 Target Object 的 StateId 纳入当前事务的 ReadSet。

### 指令三：OBJECT_DEATH (对象消亡)

OBJECT_DEATH 是对象生命周期的终点，必须保证彻底销毁且清理所有遗留效应。

#### 物理保证：

1. 级联回收 (Cascade Revoke)：自动递归撤销该 Object 发出或转交的所有 Capability。

2. 拓扑断开 (Topology Tear-down)：原子抹除所有关联的 LinkTopology 边。依赖它的对象再次尝试通过 LINK 访问时，直接触发 NullObjectReference ABORT。

3. 副作用清理：取消事务队列中所有未 COMMIT 的该对象 Effect；已 COMMIT 待执行的 Effect 强行携带该对象 Death 的快照标记。

4. 状态封印：所属 StateRoot 下的所有 StateId 标记为 Tombstone（墓碑状态），物理回收内存空间。

#### ABORT 物理复活 (Resurrection Guarantee)：

若包含 OBJECT_DEATH 的事务发生 ABORT：对象及其所有 Cap、State、Link 完整复活，恢复至死亡前的完全一致状态。

## 三、与 Veritas 内核原语的物理对齐

READ 和 WRITE：
所有 StateId 必须属于某个 ObjectId 的 StateRoot。直接读写无主 State 将被内核拒绝。

ENUM_SCOPE：
Scope 的成员列表从孤立 StateId 集合物理升级为 ObjectId 动态集合。ENUM_SCOPE 会自动展开并追踪该 Scope 下所有 Object 的结构版本号。

GRANT 和 REVOKE：
能力的持有者（Holder）必须是 ObjectId。REVOKE 触发时，结合 Object 的 CapabilitySet 进行递归图遍历撤销。

EFFECT：
每条副作用必须绑定 producer: ObjectId。死对象无法发射新 Effect。

WAL 和 RECOVER：
WAL 补充 ObjectBirth 与 ObjectDeath 二进制条目，恢复时优先重构 Object 骨架，再填充 State 数据。

## 四、数据模型扩展（Rust types.rs 落地指南）

在 types.rs 中新增以下核心结构体定义：

Runtime Object 全局唯一标识：
pub type ObjectId = u64;

Veritas 抽象机器基本粒子：
pub struct RuntimeObject {
    pub id: ObjectId,
    pub path: String,
    pub state_root: Vec<StateId>,
    pub capability_set: HashSet<CapabilityId>,
    pub contract_set: Vec<ContractDefinition>,
    pub link_topology: Vec<LinkEdge>,
    pub is_alive: bool,
}

拓扑边定义：
pub struct LinkEdge {
    pub target: ObjectId,
    pub relation: RelationKind,
    pub established_tx: TxId,
}

关系类型枚举：
pub enum RelationKind {
    CapabilityDelegation,
    ContractDependency,
    EffectPropagation,
}

WAL 条目扩展：
pub enum WALEntry {
    // 已有的 Commit, EffectAck, Checkpoint
    ObjectBirth {
        tx_id: TxId,
        object_id: ObjectId,
        path: String,
    },
    ObjectDeath {
        tx_id: TxId,
        object_id: ObjectId,
    },
}

## 五、下一步行动项 (Roadmap)

1. 确定《Veritas Runtime Object 规范 V1.0》为机器标准。
2. 重构 types.rs：加入 RuntimeObject、ObjectId 及关联 WAL 数据结构。
3. 实现内核指令 OBJECT_BIRTH：
   在 TransactionContext 的 WriteSet 中暂存对象创建。
   在 COMMIT 临界区将对象写入全局 ObjectRegistry。
   编写并发 Birth 冲突与 ABORT 零痕迹单元测试。
4. 升级 Scope 机制：将 ScopeRegistry 从单纯管理 State 升级为管理 Object 拓扑。
5. 实现 OBJECT_LINK 与 OBJECT_DEATH。
