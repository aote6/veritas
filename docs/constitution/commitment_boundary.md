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

## 7. Q1-Q4 Architecture Decisions

Phase 1 architecture questions have now been formally adjudicated.

### Q1 — tx_id in WorldSnapshot

Decision: REMOVE

tx_id is not World State and is not part of Delta Identity.

Evidence:

- docs/constitution/commit_version.md explicitly defines tx_id as a process identifier and excludes it from World State and Delta Identity.
- restore_checkpoint() has no consumer for snap.tx_id.
- create_checkpoint() only writes the current transaction ID into the snapshot; no restoration or identity verification path reads it.
- Therefore WorldSnapshot.tx_id has no continuation or identity semantics.

Decision:

WorldSnapshot.tx_id shall be removed. This is a semantic cleanup, not an identity-model change. Implementation must still verify the complete repository impact because WorldSnapshot is a checkpoint/serialization boundary.

### Q2 — commitment_hash naming

Decision: RENAME to state_commitment

The field currently represents only the State Identity / Commitment Domain:

StateStore + ObjectRegistry + Topology + CapabilityGraph + ScopeRegistry -> root_hash() -> commitment_hash

It does not represent:

- global_version
- object_id_counter
- grant_sequence
- last_applied_delta_hash
- tx_id

Therefore commitment_hash is semantically misleading.

Decision:

WorldSnapshot.commitment_hash shall be renamed to state_commitment. This rename is part of Phase 2A and does not change the State Commitment algorithm.

### Q3 — Hash migration order

Decision: State Commitment first

The two hash lines shall migrate independently.

Migration order:

Phase 2A structural cleanup: tx_id removal + commitment_hash to state_commitment

Phase 2B State Commitment hash migration: FNV-1a u64 to SHA-256 or BLAKE3-256

Phase 2C checkpoint State Commitment verification

Phase 2D Delta Identity hash migration: FNV-1a to SHA-256 or BLAKE3-256

Rationale:

State Commitment is the primary externally meaningful representation of World State and is exposed through checkpoint/receipt/state-root related paths. Delta Identity participates directly in version continuity, replay acceptance, equal-version detection, WAL continuity, and checkpoint continuation. It therefore has a larger behavioral coupling surface.

Decision:

State Commitment migration and Delta Identity migration must remain independent changes. No combined global hash upgrade is permitted.

### Q4 — Checkpoint commitment verification

Decision: verify after restoration, before accepting the checkpoint

restore_checkpoint() shall:

1. restore the checkpoint continuation metadata and five State components;
2. recompute the State Commitment from the restored five-component state;
3. compare the recomputed commitment with the checkpoint stored state_commitment;
4. return failure if the commitments differ;
5. only accept the restored checkpoint when the commitment matches.

Conceptually:

WorldSnapshot carries state components and state_commitment.
After restoration, recompute root from restored five components.
Compare recomputed root with state_commitment.
Match -> accept. Mismatch -> reject.

The verification is intentionally performed after restoration, because the commitment is defined over the resulting five-component State Domain.

Decision:

restore_checkpoint() shall not accept a checkpoint whose restored State Commitment differs from the checkpoint declared state_commitment. This verification is implemented in Phase 2C, after the State Commitment algorithm migration in Phase 2B.

## 8. Revised Phase 2 Execution Order

The accepted architecture therefore establishes the following implementation sequence:

### Phase 2A — Snapshot Identity Cleanup

- Remove WorldSnapshot.tx_id.
- Rename WorldSnapshot.commitment_hash to state_commitment.
- Update all constructors, serializers, tests, documentation, and APIs.
- Do not change hash semantics.
- Do not modify root_hash().

### Phase 2B — State Commitment Hash Migration

- Replace the current FNV-1a State Commitment implementation.
- Adopt a 256-bit cryptographic hash.
- Update root_hash() / State Commitment consumers.
- Update Receipt before_root / after_root.
- Update replay equivalence.
- Update state_root API.
- Update Forge/WRI exposure and related tests.
- Establish the new canonical State Commitment representation.

### Phase 2C — Checkpoint Commitment Verification

After Phase 2B is stable:

- recompute State Commitment after checkpoint restoration;
- compare against state_commitment;
- reject mismatched checkpoints;
- add RED tests for corrupted State Commitment;
- add GREEN regression tests for valid checkpoint restoration.

### Phase 2D — Delta Identity Hash Migration

Independently migrate:

- TransactionDelta::content_hash();
- delta_content_hash();
- last_applied_delta_hash.

Verify:

- equal-version Delta discrimination;
- replay continuity;
- WAL continuity;
- checkpoint continuation;
- Receipt / Delta Identity consumers.

## 9. Final Architecture Constraints

Effective immediately, the following constraints are accepted:

1. WorldSnapshot.tx_id is not part of the Veritas checkpoint identity model and shall be removed.
2. WorldSnapshot.state_commitment represents State Identity only.
3. State Identity is defined exclusively by the five-component Commitment Domain.
4. global_version, object_id_counter, grant_sequence, and last_applied_delta_hash are Continuation Metadata and shall not enter the State Commitment Domain.
5. tx_id shall not enter State Identity, Delta Identity, or Continuation Identity.
6. State Commitment and Delta Identity are independent cryptographic identity lines.
7. Their hash migrations shall be implemented and verified independently.
8. State Commitment migration precedes Delta Identity migration.
9. restore_checkpoint() shall verify the restored State Commitment against the checkpoint declared state_commitment.
10. No implementation may add Continuation Metadata to root_hash() unless this ADR is formally revised.
11. No implementation may merge the State Commitment and Delta Identity migrations into a single architectural change.

## 10. Revision Conditions

This decision must be reopened if any of the following occurs:

- the Constitution changes the definition of World State;
- the five-component Commitment Domain changes;
- canonical_identity_bytes() changes its canonical encoding contract;
- new State components are introduced;
- new Continuation Metadata is introduced;
- the semantics of global_version or last_applied_delta_hash change;
- checkpoint restoration semantics change;
- new code evidence contradicts the Phase 0 audit.

Until such a revision occurs, this document is the authoritative boundary for State Identity, Delta Identity, and Continuation Identity.

## 11. Commitment Algorithm Versioning

### 11.1 原则

State Commitment 的语义永久不变：

State Commitment = H(canonical five-component World State)

但 H 的具体算法不绑定在 World State 语义中。算法是可替换的密码学边界。

### 11.2 当前算法

Phase 2B 起，State Commitment 采用：

- 算法：SHA-256
- 输出：[u8; 32]
- 实现：手写，无外部依赖，位于 src/crypto.rs
- 算法版本号：1

### 11.3 算法版本字段

WorldSnapshot 增加：

commitment_algorithm: u8

当前值 1 = SHA-256。

未来引入新算法时：

- 2 = SHA-512/256 或其他已裁定算法
- state_commitment 字段宽度随算法版本变化
- restore / verify 必须先读 commitment_algorithm，再选对应哈希函数

### 11.4 迁移规则

- 禁止在同一 WorldSnapshot 中混用两种 commitment 算法。
- 禁止在未更新 commitment_algorithm 的情况下改变哈希算法。
- 禁止在未正式修订本 ADR 的情况下新增算法版本。
- 算法升级只影响 State Commitment 的生成与验证，不改变五组件语义。
- 旧 checkpoint 在算法升级后不再可验证，除非提供显式迁移路径。

### 11.5 为什么不是 BLAKE3

BLAKE3 在无依赖约束下自写实现约 500 行，chunk 状态机和边界处理复杂度高，测试向量少。SHA-256 自写约 250 行，算法简单，官方测试向量极多，跨平台验证生态最成熟。Veritas 无外部协议要求必须使用 BLAKE3。

### 11.6 为什么不是 SHA-512

SHA-512 提高了量子碰撞余量，但代价是 state_commitment 字段宽度从 32 变 64，所有 Receipt / checkpoint / replay 结构迁移成本高。当前威胁模型下 SHA-256 的碰撞安全性已足够。真正的长期量子风险在签名/认证层，不在 State Commitment 层。
