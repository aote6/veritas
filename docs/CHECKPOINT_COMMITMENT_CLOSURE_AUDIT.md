# Checkpoint Integrity / Commitment Closure — 代码事实审计

**日期**: 2026-08-20
**状态**: 审计完成
**范围**: root_hash / state_root / global_version / last_applied_delta_hash 的生成→存储→更新→checkpoint→restore→verify 全链

---

## 1. 当前代码事实

### 1.1 关键字段与生成方式

| 字段 | 生成方式 | 存储位置 | 更新时机 |
|---|---|---|---|
| root_hash() | SHA-256 over 五组件（StateStore, ObjectRegistry, Topology, CapabilityGraph, ScopeRegistry） | 计算得出，无独立存储 | 每次调用时重新计算 |
| state_root() | 等于 root_hash()（直接委托） | 无独立存储 | 同 root_hash() |
| global_version | AtomicU64 | VeritasEngine | apply() 中更新为 delta.commit_version |
| last_applied_delta_hash | delta.content_hash()（SHA-256 over canonical_identity_bytes） | Mutex<[u8; 32]> | apply() 中更新 |
| state_commitment（snapshot 字段） | 等于 root_hash() | WorldSnapshot | create_checkpoint() 时填充 |

### 1.2 关键关系

state_root() ≡ root_hash()          （同一函数，完全等价）
state_commitment ≡ root_hash()      （checkpoint 时写入）
last_applied_delta_hash = SHA-256(canonical_identity_bytes(delta))

### 1.3 last_applied_delta_hash 的生成

delta.content_hash()
  = delta_content_hash(canonical_identity_bytes(delta))
  = SHA-256(canonical_identity_bytes(delta))

canonical_identity_bytes() 不包含 tx_id 和 commit_version，只包含 actor_id 和所有语义 mutation 字段。

### 1.4 global_version 的更新

commit_version = global_version.load() + 1
delta = build_delta(ctx, ..., commit_version)
apply(delta):
    global_version.store(delta.commit_version)
    last_applied_delta_hash = delta.content_hash()

注意：global_version 和 last_applied_delta_hash 在 apply() 中同时更新，但二者没有相互绑定关系。

### 1.5 Checkpoint 的验证逻辑

restore_checkpoint(snap) 中：

1. 验证 commitment_algorithm == 1
2. 从 snap 的五组件重新计算 state_commitment
3. 比较计算值与 snap.state_commitment
4. 验证通过后，恢复 global_version、object_id_counter、grant_sequence、last_applied_delta_hash
5. 不验证 last_applied_delta_hash 与 global_version 之间的任何关系
6. 不验证 last_applied_delta_hash 是否真的对应 global_version 所指的那个 delta

---

## 2. 已覆盖的测试

### 2.1 tests/commitment_boundary.rs（3 tests）

- state_commitment_excludes_global_version：验证 state commitment 不包含 global_version
- checkpoint_preserves_last_applied_delta_hash：验证 checkpoint 保留 delta hash
- delta_identity_independent_of_state_commitment：验证 delta identity 与 state commitment 相互独立

### 2.2 tests/security_recovery_audit.rs（5 tests）

- audit_last_applied_delta_hash_genesis_is_zero：genesis 时 delta hash 为零
- audit_last_applied_delta_hash_updates_on_apply：apply 后 delta hash 更新
- audit_checkpoint_preserves_last_applied_delta_hash：checkpoint 保留 delta hash
- audit_checkpoint_roundtrip_identity_continuity：roundtrip 后 identity 连续
- 另有 audit_checkpoint_preserves_last_applied_delta_hash 的变体

### 2.3 tests/checkpoint_continuity.rs（4 tests）

验证五组件的连续性，但不涉及 delta hash 的 commitment closure。

---

## 3. 审计发现

### Gap 1：last_applied_delta_hash 与 global_version 无绑定验证

**当前状态**：restore 后，last_applied_delta_hash 和 global_version 分别被恢复，但没有任何验证确保它们来自同一个 delta。

**攻击/故障场景**：
- 篡改 snap.last_applied_delta_hash，restore 后 global_version 仍指向原 version，但 delta hash 已被替换。
- 没有任何机制能检测到这种不一致。

**严重性**：中等。这是 Continuation Metadata 内部的 consistency gap。

**状态**：⚠️ 待 Constitution §3.4 确认是否要求 cryptographic binding

### Gap 2：content_hash() 不包含 commit_version

**当前状态**：canonical_identity_bytes() 明确排除了 tx_id 和 commit_version。

**含义**：两个不同 version 但语义 mutation 完全相同的 delta，会产生相同的 content_hash()。

**严重性**：低-中。取决于 Constitution 对 Delta Identity 的精确定义。

**状态**：⚠️ 保留，但不现在修。需要确认 Constitution 是否要求 content_hash 本身是版本唯一标识，还是 (version, content_hash) 二元组才是 Delta Identity。

### Gap 3：restore_checkpoint 不验证 state_commitment 与 last_applied_delta_hash 之间的历史一致性

**当前状态**：state_commitment 和 last_applied_delta_hash 在 WorldSnapshot 中是两个独立字段，restore 时分别验证/恢复。

**严重性**：无。这是设计边界（ADR commitment_boundary.md 明确将 State Identity 和 Continuation Identity 解耦）。

**状态**：✅ 关闭，不实施。强行绑定反而违反 ADR。

---

## 4. 最终状态

### 4.1 已正确落地的部分

- State Commitment（五组件 SHA-256）：完全闭合
- root_hash → state_root → state_commitment：等价链确认
- checkpoint 写入和 restore 重算验证：闭合
- Continuation Metadata 的 checkpoint 持久化：闭合
- ADR commitment_boundary 的设计意图：正确实现

### 4.2 剩余 Gap

Gap 1 是唯一可能需要对代码修改的缺口，但取决于 Constitution §3.4 的精确要求：

- 方案 A：不修。Constitution 允许 global_version 和 last_applied_delta_hash 作为独立 metadata 存在。
- 方案 B：新增 continuation_commitment 字段，将 (global_version, last_applied_delta_hash) 做 cryptographic binding。
- 方案 C：仅增加 sanity check，验证 version == 0 ↔ hash == ZERO_HASH。

方案 C 只解决初始态一致性，不是 Gap 1 的完整修复。

### 4.3 下一步

只读 Constitution §3.4 和相关条款，确认：
1. Continuation Metadata 是否要求 cryptographic binding？
2. Delta Identity 是 content_hash 还是 (version, content_hash)？

根据答案决定 Gap 1 是否成为红测试目标。
## 5. Constitution 条款审计结论（2026-08-20 补充）

### 5.1 关键条款

**commit_version.md §3.2**：
Delta Identity = (commit_version, delta_content_hash)

**commit_version.md §3.4**：
global_version 和 last_applied_delta_hash 是不可分割的 World State 元组，必须一起进入 WorldSnapshot 和 Checkpoint。
**不能出现 version = N 但 hash = H(N-1) 的状态。**

**commitment_boundary.md §2.2**：
Continuity Version Identity = (global_version, last_applied_delta_hash)
其中 global_version 是位置，last_applied_delta_hash 是内容身份。两者联合才能唯一定位世界演化序列中的一步。

**commitment_boundary.md §2.3**：
Continuation Identity = (global_version, object_id_counter, grant_sequence, last_applied_delta_hash)
这些字段让恢复后的 Machine 能够继续运行。

### 5.2 对 Gap 1 的裁定

§3.4 明确要求：**不能出现 version = N 但 hash = H(N-1) 的状态**。

当前 `restore_checkpoint()` 不验证这个约束。这意味着：
- 一个被篡改的 checkpoint 可以携带 version=42 和 hash=H(7)（或任意值）
- restore 后系统会静默接受这个不一致状态
- 这违反了 Constitution §3.4

**Gap 1 是 Constitution 级的缺口，需要实施修复。**

### 5.3 对 Gap 2 的裁定

§3.2 明确定义 Delta Identity = (commit_version, delta_content_hash)。

content_hash 不包含 commit_version 是**宪法要求**（§3.3 第一条：必须排除 commit_version）。

Gap 2 不是缺陷，而是正确的设计。

### 5.4 对 Gap 3 的裁定

commitment_boundary.md §2.3 明确将 State Identity 和 Continuation Identity 解耦。

Gap 3 不是缺陷，而是设计边界。

### 5.5 最终裁定

需要修复的只有 Gap 1。修复方向：

在 `restore_checkpoint()` 中增加验证：恢复后检查 `(global_version, last_applied_delta_hash)` 的一致性。

但具体验证方式受 commitment_boundary.md §4（约束 4）限制：
- 原约束："在 Q4 裁定之前，restore_checkpoint() 不得增加 commitment 验证逻辑"
- Q4 已裁定：verify after restoration, before accepting the checkpoint

因此可以在 restore_checkpoint() 的现有 State Commitment 验证之后，增加 Continuation Consistency 验证。
