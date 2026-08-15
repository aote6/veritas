# Commitment Boundary Decision

Status: ACCEPTED
Phase: 1 — Architecture Decision
Predecessor: Phase 0 Read-Only State/Commitment Audit
Date: 2026-08-15

## 1. 背景

Phase 0 只读审计确认了以下代码事实：

- `commitment_hash` 的当前实现是 `root_hash()` 的零扩展，不是整个 `WorldSnapshot` 的哈希。
- `root_hash()` 只覆盖五个 State 组件（StateStore、ObjectRegistry、Topology、CapabilityGraph、ScopeRegistry）。
- `last_applied_delta_hash` 由 `TransactionDelta::content_hash()` 产生，是独立于 `root_hash()` 的第二条哈希线。
- `global_version`、`object_id_counter`、`grant_sequence`、`last_applied_delta_hash` 均不进入 `root_hash()`。
- `tx_id` 存在于 `WorldSnapshot` 中，但不进入任何哈希。

这些事实表明，Veritas 当前实际存在三个不同语义层级的 identity，而现有字段名和文档未能清晰区分它们。本裁定正式固定这个边界。

## 2. 三种 Identity 的正式定义

### 2.1 State Identity

State Identity 回答的问题是：当前世界的五个 State 组件是什么？

定义：

State Identity = H(
    StateStore,
    ObjectRegistry,
    Topology,
    CapabilityGraph,
    ScopeRegistry
)

当前实现：

- root_hash() -> u64
- commitment_hash -> [u8; 32]（当前为 root_hash() 的零扩展，不是独立的 256-bit cryptographic commitment）

State Identity 覆盖的五个组件统称为 Commitment Domain。

### 2.2 Delta Identity

Delta Identity 回答的问题是：世界演化的最后一步是谁？

定义：

Delta Identity = H(canonical(TransactionDelta))

当前实现：

- TransactionDelta::canonical_identity_bytes() -> Vec<u8>
- delta_content_hash() -> [u8; 32]
- last_applied_delta_hash -> Mutex<[u8; 32]>

global_version 与 last_applied_delta_hash 共同构成版本连续性身份：

Continuity Version Identity = (global_version, last_applied_delta_hash)

其中 global_version 是位置，last_applied_delta_hash 是内容身份。两者联合才能唯一定位世界演化序列中的一步。

### 2.3 Continuation Identity

Continuation Identity 回答的问题是：恢复后的世界能否继续运行？

定义：

Continuation Identity = (
    global_version,
    object_id_counter,
    grant_sequence,
    last_applied_delta_hash
)

这些字段是 checkpoint 恢复所需的 Continuation Metadata。

它们的作用不是描述五组件的内容，而是让恢复后的 Machine 能够：

- 继续接受下一个 commit_version
- 继续分配 ObjectId
- 继续分配 Capability sequence
- 继续判断 equal-version Delta 的合法性

关键裁定：Continuation Metadata 不进入 State Commitment Domain。

这是本决策最重要的边界。


注：commit_version.md §7 将 last_applied_delta_hash 称为 World State。本 ADR 不否定这一点，而是将 World State 细分为 Commitment Domain（进入 State Identity）与 Continuation Identity（不进入 State Identity）。两者都是 World State 的组成部分，都必须在 checkpoint 中保存恢复。

## 3. 三种 Identity 的关系

世界快照包含两个 domain：

- State Identity domain（Commitment Domain）：StateStore、ObjectRegistry、Topology、CapabilityGraph、ScopeRegistry
- Continuation Identity domain：global_version、object_id_counter、grant_sequence、last_applied_delta_hash

State Identity 通过 root_hash() 计算，当前由 commitment_hash 字段承载。

Continuation Identity 不进入 root_hash()，也不进入 commitment_hash。

Process Metadata（tx_id）不属于上述任何 Identity，是独立的过程标识。

Delta Identity 是独立线：

TransactionDelta -> canonical_identity_bytes() -> delta_content_hash() -> last_applied_delta_hash

last_applied_delta_hash 与 global_version 联合构成 Continuity Version Identity，同时作为 Continuation Identity 的一部分被 checkpoint 保存。

## 4. commitment_hash 字段的当前定位

WorldSnapshot.commitment_hash: [u8; 32] 保留。

其当前正式语义为：

该字段承载 State Identity（State Commitment），不是整个 WorldSnapshot 的完整身份。

未来可以改名为 state_commitment 或 content_root_hash，但 Phase 1 不做任何字段修改。

## 5. tx_id 的定位

tx_id 是过程/交易标识。它：

- 不进入 State Identity
- 不进入 Delta Identity
- 不进入 Continuation Identity

至于 WorldSnapshot 的 Serialization Contract 是否应该携带 tx_id，作为独立的 serialization-contract question 记录，不在本决策中解决。

## 6. Hash 算法迁移策略

当前两条哈希线都使用 FNV-1a u64，密码学强度不足。

但迁移必须拆分为两条独立线路：

### 6.1 State Commitment Hash Migration

影响面：

- root_hash()
- commitment_hash
- Receipt before_root / after_root
- replay equivalence
- state_root API
- Forge/WRI 暴露
- root_hash 相关测试

### 6.2 Delta Identity Hash Migration

影响面：

- delta_content_hash()
- last_applied_delta_hash
- equal-version detection
- checkpoint continuation
- WAL/replay continuity

两条迁移线可以独立执行、独立验证。禁止合并为单一大 Hash 升级。

## 7. 未决问题

| # | 问题 | 状态 |
|---|---|---|
| Q1 | WorldSnapshot 的 Serialization Contract 是否应包含 tx_id | 未裁定 |
| Q2 | commitment_hash 字段最终命名 | 未裁定，倾向 state_commitment |
| Q3 | State Commitment 和 Delta Identity 的算法迁移顺序 | 未裁定，建议 State Commitment 优先 |
| Q4 | restore_checkpoint() 应在恢复前还是恢复后验证 commitment | 未裁定，依赖 Q3 |

## 8. 本决策的约束力

自本决策 ACCEPTED 起：

1. 禁止将 commitment_hash 解释为整个 WorldSnapshot 的哈希。
2. 禁止将 global_version、object_id_counter、grant_sequence、last_applied_delta_hash 加入 root_hash()，除非本决策被正式修订。
3. 禁止将 State Commitment 与 Delta Identity 的算法迁移合并为单一变更。
4. 在 Q4 裁定之前，restore_checkpoint() 不得增加 commitment 验证逻辑。

## 9. 修订条件

以下任一情况发生时，本决策需要重新开启：

- Constitution world.md 或 commit_version.md 的 World State 定义发生变更
- canonical_identity_bytes() 的编码规则发生变更
- 引入新的 State 组件或 Continuation Metadata 字段
- 发现新的代码事实推翻 Phase 0 审计结论
