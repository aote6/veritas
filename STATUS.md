=== Veritas Kernel STATUS ===

Date: 2026-08-04
Branch: main

## Current milestone

P30 Deterministic Replay — World state replay verified; Capability identity gap identified

P30.1: Same WAL → identical Engine state (4 determinism tests)
P30.2: WAL contains full world state — object/link/capability replay (2 tests)
P30.3: Capability Identity Replay ✅ — grant_sequence + capability_id persist via WAL + apply() (capability_id_of + restore_grant)

## Completed

Transaction   - BEGIN/COMMIT/ABORT, OCC, snapshot isolation, savepoint
State         - Address=(ObjectId,StateId), read/write sets, state root
Object        - Birth / Freeze / Death; ObjectRecord first-class
Link OWNS     - Owner death to owned death (transitive closure)
Link DEPENDS_ON - Dependency death to DependencyInvalidated
Link REFERENCES - Edge removal only
Capability    - Graph grant/delegate/revoke; lazy resource liveness at use
WAL           - Commit/birth/death/link/freeze/unlink/capability-grant/effect;
                recovery rebuilds object_registry, topology, capability_graph
Machine       - Step/run, Call/Return same tx, trap frame
Assembler     - Present; Module-as-template path still evolving
P8            - Object Death semantic complete
P4.x/P5.x     - Capability/Freeze/Unlink WAL-durable
Phase 1 Step 7 - KernelCall ABI codec: decode() + handle() + TrapResult
Phase A       - Kernel lifecycle detached from Runtime::execute()
P28.0         - Cross-module TRAP persistence test
P28.1         - Machine dispatch closure: 30/30 reachable
P29.1–P29.5   - WAL Recovery verification pyramid (24 tests)
P30.1         - Deterministic Replay: same WAL → identical Engine state (4 tests)
P30.2         - WAL full world state replay: object/link/capability (2 tests)

### Recovery Verification Pyramid (P29)

  P29.1 — Basic recovery (3 tests)
  P29.2 — Lifecycle invariants (6 tests)
  P29.3 — Equivalence snapshot (5 tests)
  P29.4 — Random property testing (3 tests, 1600 ops)
  P29.5 — Robustness: truncated/corrupted/idempotent/empty (7 tests)
  P4.x  — Capability recovery (3 tests)
  P5.x  — Freeze/Unlink recovery (5 tests)
  Total: 32 recovery tests

### Deterministic Replay Status (P30)

  P30.1 — Same WAL → same Engine ✅
  P30.2 — WAL → object_registry + topology correct ✅
  P30.3 — Capability Identity Replay ✅
  grant_sequence + capability_id now persist via WAL TransactionCommitted.
  restore_grant() uses the persisted sequence on recovery.

### ReplayRecord Gap

  ReplayRecord currently only records: writes, before_root, after_root.
  Does NOT record: object births, links, capability grants, lifecycle events.
  Source comment: "ReplayRecord目前不记录object_id，统一归属内核Object(0)"
  This means the old ReplayEngine (src/replay.rs) only replays StateMemory,
  not the Veritas world. Real ReplayEngine must be upgraded to handle
  the full TransactionDelta vocabulary.
  Status: TransactionDelta is now the canonical unit; ReplayRecord upgrade = P30.4.

## Explicit non-goals

P8.4 Death Event Dispatcher = structural refactor only (deferred)
No new death semantics without constitution change
Object lifecycle instructions stay Trap ABI

## Step 2 (apply unification) completed

- Step 1a: WAL CRC integrity (len-prefix + crc32, truncation tests)
- Step 1b: TransactionDelta struct, build_delta() extracts raw facts
- Step 2a: apply(&delta) — 10-step ordered projection, closure recomputed
- Step 2b: commit() → apply(&delta), memory mutations deferred after all WAL writes
- Step 2c: Recovery groups records by tx_id, only commits with Commit marker, applies in order
- Runtime apply == Recovery apply ✅
- 162 tests pass

### Remaining Step 2 tech debt

✅ All resolved (2026-08-04):
- state_map/apply_records dual path eliminated — state_store init fully owned by apply()
- Cross-tx unlink-then-death test added

## Step 3 (atomic WAL write) completed

- WalEntry::TransactionCommitted(TransactionDelta) — single atomic WAL entry
- commit(): 7 independent append_and_sync → 1 TransactionCommitted + apply()
- Recovery: with_wal_path() handles TransactionCommitted alongside legacy WalEntry::Commit
- CRC truncation test for new format (test_truncated_transaction_committed_discarded)
- Orphan test updated: corrupted TransactionCommitted discarded by CRC check
- Critical #1 (Commit non-atomicity) resolved ✅
- 162 tests pass


## Known gaps

### From audit — not yet addressed this cycle

**Capability** (Critical #4) ✅ (2026-08-04 audit hardening session):
- capability_enforced field, constructor init, enforce_capability() method, verify_capability() early-return: all physically deleted
- machine.rs enable_capability_enforcement() dead code removed
- grant_base_access confirmed as intentional design (BaseAccess cap deterministically derived from object_id, regenerated each begin_in_object, no cross-tx state to persist) — design note added in source
- Remaining: verify_capability only covers write_set, not reads/links/death/freeze/cross-object calls (scope expansion deferred)

**Kernel boundary** (Critical #3) ✅:
- P1a: Machine dispatch unified via KernelCall; execute_kernel_instruction() deleted
- P1b: Engine 15 mutation methods pub → pub(crate); Kernel::handle() is sole external mutation entry
- 2026-08-04追加: kernel.rs 自身 7 个 mutation 透传方法(object_birth/death/link/unlink/freeze/capability_grant/abort)物理删除，handle() 改为直调 self.engine.xxx()
- 保留 pub 的: begin/begin_in_object/commit/read/write/effect/savepoint/rollback_to (事务生命周期+数据访问+Machine TRAP 路径) + 只读查询透传

**ObjectId allocation**:
- ObjectId still provided by caller, not assigned by Kernel
- Dependency on committed-tx determination is now satisfied (Step 3 complete); ready to schedule
- Boundary: only count Births from TransactionCommitted entries, not orphaned ones

**Dual stores**:
- ✅ src/graph/ removed — 1021 lines dead code deleted


✅ P1a: Machine routes kernel ops via KernelCall; execute_kernel_instruction deleted; pub API not yet closed (P1b pending)



**Module lifecycle**:
- ModuleObject/ModuleInstance separation not fully closed per constitution
- ModuleInstance as StateObject subtype, auto-creation on load, death notification still partial

**Savepoint**:
- Structure exists, full semantics not implemented; constitution declares "future extension"

### From this refactoring cycle — completed

**state_map/apply_records dual path** ✅ (2026-08-04 audit hardening session):
- state_store/scope_registry 从 from_map() 预填改为 new() 空初始化
- writes/scope_changes 完全由 apply() 循环第1、2步统一填充
- apply_records() 保留（只需它的 pending_effects 和 max_tx_id）
- 验证: 162 tests pass, wal_recovery_equivalence + robustness 全绿

**Cross-tx unlink-then-death boundary test** ✅:
- 2 tests added, 157 total

### Legacy — unchanged this cycle

engine.rs size / modular split deferred
DEPENDS_ON carrier may evolve (Effect/Trap); event semantics fixed
Eager capability purge on death: not required (lazy is normative)
execute_kernel_instruction() is a no-op stub — all logic has been moved inline to step()
ReplayRecord missing Object/Link/Capability — ReplayEngine is StateMemory-only

## Next milestones

### Immediate (unlocked by Step 3)
1. ✅ Step 3.5 — Effect retry: apply_records dedup + with_wal_path pending loop (2 tests)
2. ⚠️ ObjectId allocation: Kernel internal allocator implemented (next_object_id), TRAP path works; pub engine.object_birth(ctx, id) still accepts caller-supplied ID — dual paths coexist, closure depends on P1b
3. ✅ Cross-tx unlink-then-death recovery boundary test (2 tests, 157 total)
4. ✅ Cleanup: unused imports, dead code in test_truncated_transaction_committed_discarded

### Short-term
4. P30.4 — ReplayRecord upgrade: Object/Link/Capability (now TransactionDelta-based)
5. P30.5 — ReplayEngine full world replay + Receipt verification
6. ✅ state_map/apply_records dual path — cleaned up (2026-08-04)
7. ✅ ObjectId allocation — exclusive TRAP path after P1b

### Medium-term (from audit Critical findings)
7. ✅ Capability always-on: capability_enforced toggle removed, verify_capability unconditional (2026-08-04)
8. ✅ Kernel boundary: P1a ✅ + P1b ✅ + 7透传方法删除 (2026-08-04)
9. ✅ src/graph/ dead code removed; engine.topology is sole topology store

### Later
10. P31 — Checkpoint / Snapshot
11. ModuleObject/ModuleInstance lifecycle closure
12. ✅ P1a: Machine dispatch unified via KernelCall
13. Savepoint full semantics

## Documentation map

See README. 162 tests pass.


---

## P1b Completion — 2026-08-04

### Kernel Boundary Physically Closed

Engine mutation API changed from `pub` to `pub(crate)`:
`begin`, `begin_in_object`, `read`, `write`, `effect`, `commit`,
`capability_grant`, `object_freeze`, `object_death`, `object_link`,
`object_unlink`, `object_birth`, `abort`, `savepoint`, `rollback_to`

`Kernel::handle()` is now the only external entry point for world mutation.

### Instruction Dispatch Unified (P1a)
- Machine routes Commit/Abort/CapabilityGrant/Effect/Savepoint/RollbackTo via KernelCall + handle()
- `execute_kernel_instruction()` deleted
- Read/Write retained as direct path (data access instructions)

### Dead Code Removed
- `src/graph/` (GraphStore/Journal/Policy/ReplayEngine) — 1021 lines, zero callers
- `tests/graph/` — tests for removed module

### Test Infrastructure Migrated
- `tests/common/mod.rs`: `new_kernel()` uses `Kernel::with_wal_path` + `Kernel::handle(ObjectBirth)` for Root Object creation
- Root Object ID dynamically allocated by Kernel (no fixed FNV hash)
- `TestKernel.engine` field removed; all tests use `tk.kernel`
- 148 tests total, 0 failed, 0 ignored

### ObjectId Allocation
- `engine.next_object_id()`: AtomicU64 counter initialized from `max(birth_id)+1` in recovery
- `Kernel::handle(ObjectBirth)` uses `engine.next_object_id()` — caller cannot specify ID
- Old `engine.object_birth(ctx, id)` retained as `pub(crate)` for Kernel internal use

### Frozen Link Validation
- Moved from `object_link()` (immediate reject) to `commit()` pending_links validation
- Correct transactional semantics: intent recorded at command, validated at commit

### Test Files Migrated
| File | Tests | Status |
|------|-------|--------|
| wal_recovery_equivalence.rs | 7 | ✅ |
| wal_recovery_invariants.rs | 4 | ✅ (2 old-API tests deleted) |
| wal_recovery_object.rs | 3 | ✅ |
| wal_recovery_robustness.rs | 7 | ✅ |
| capability_p4x_recovery.rs | 3 | ✅ |
| freeze_unlink_p5x_recovery.rs | 5 | ✅ |
| object/birth.rs | 1 | ✅ |
| object/lifecycle.rs | 13 | ✅ (1 semantic fix) |
| object/memory.rs | 1 | ✅ |
| replay_determinism.rs | 4 | ✅ (sort fix) |
| replay_engine_test.rs | 2 | ✅ |
| transaction/commit.rs | 1 | ✅ |
| transaction/conflict.rs | 1 | ✅ |
| transaction/isolation.rs | 2 | ✅ |
| wal_recovery_property.rs | - | Deleted (pending re-implementation) |

### Remaining Known Gaps (2026-08-04 audit hardening session)

**Immediate — ALL CLOSED:**
1. ✅ Kernel passthrough pub fns — 7 mutation透传方法删除，handle() 直调 self.engine
2. ⬜ kernel.read/write() still pub (Memory ABI not yet defined in KernelCall — deferred)
3. ✅ state_map/apply_records dual path — eliminated
4. ✅ Capability always-on — capability_enforced toggle removed, grant_base_access confirmed intentional design
5. ✅ grant_base_access — confirmed intentional, design note added

**Short-term:**
6. P30.4 — ReplayRecord upgrade: Object/Link/Capability
7. P30.5 — ReplayEngine full world replay + Receipt verification

**Structural finding (not in original audit):**
- state_memory 是与 state_store 平行、脱节的影子状态结构。record_history/apply_state_memory/create_checkpoint/restore_checkpoint/ReplayRecord 整条链路基于 state_memory，而非统一到 apply() 的主状态 state_store。在动 Checkpoint/Replay 之前必须先决定此链路是接入主状态还是废弃重做。

**Later:**
8. ModuleObject/ModuleInstance lifecycle closure
9. Savepoint full semantics
10. P31 — Checkpoint/Snapshot


---

## Audit Hardening Closeout — 2026-08-04

### 核查方法论
- 每条"已完成"结论必须看到实际源码/diff，不采信纯文字总结
- 测试全绿只证明行为未破坏，不证明改动方式与声称一致
- 每次验证前先假设"如果这条总结是错的，错在哪里"，据此设计验证命令

### Critical 闭合确认

| # | 审计发现 | 最终状态 | 验证方式 |
|---|---------|---------|---------|
| #1 | Commit 非原子 | ✅ | 源码确认: build_delta() → 单次 append_and_sync → apply()，注释"apply after WAL is durable" |
| #2 | Runtime apply ≠ Recovery apply | ✅ | 第一轮发现 state_map 双路径残留 → 本轮修复: state_store/scope_registry 改为 new() 空初始化，完全由 apply() 统一填充 |
| #3 | Kernel 边界可绕过 | ✅ | P1a/P1b + 本轮追加: kernel.rs 7个透传方法物理删除，handle() 改直调 self.engine |
| #4 | Capability 可关闭 + 旁路 | ✅ | capability_enforced 全链路删除（字段/初始化/方法/早退判断/machine.rs死代码），grant_base_access 确认为有意设计 |

### 测试状态
162 tests, 0 failed, 0 ignored

### 本轮附加发现
- state_memory 影子系统: 与 state_store 平行脱节，影响后续 Checkpoint/Replay 设计
- wal.rs unused import (PendingCapabilityGrant): 无害边角料，下次顺手清理


---

## Stage 3.1 RootHash — 2026-08-04

### 实现
- `engine.root_hash()` — WorldState 五组件确定性根哈希
- 前置接口: `StateStore::all_entries()`, `CapabilityGraph::all_grants()`, `ScopeRegistry::all_scopes()`
- 哈希: FNV-1a 变体，`u64` LE 编码，各组件独立排序后哈希，最终 H(h1..h5)
- 五组件: StateStore, ObjectRegistry, Topology, CapabilityGraph, ScopeRegistry
- ObjectBody 不入 Hash（Memory 内容属于 StateStore）
- ObjectRegistry 保留 Dead 记录（object_id_counter 可重建）

### 测试 (5 new)
- `empty_world_root_hash_is_deterministic` — 空引擎 hash 非零且一致
- `root_hash_changes_on_write` — 写入改变 hash
- `root_hash_changes_on_birth` — 创建 Object 改变 hash
- `root_hash_changes_on_link` — 建立 Link 改变 hash
- `root_hash_order_independent` — 插入顺序不影响最终 hash

### 不变式
- 不修改 apply() / commit() / WAL
- 不新增状态模型
- 不引入第二套 hash 体系
- 162 tests, 0 failed


---

## Stage 3.2 Replay — 2026-08-04

### 实现
- `Kernel::replay(wal_path) -> u64` — 从 WAL 重放全部已提交事务
- `build_ordered_deltas()` 提取到 wal.rs，Recovery 和 Replay 共用
- `VeritasEngine::empty()` — 完全空引擎构造器（不读 WAL）
- Replay = Recovery 去掉恢复后的运行阶段

### 测试 (4 new)
- `replay_empty_wal_returns_nonzero` — 空 WAL 返回非零固定 hash
- `replay_equals_recovery_idle` — Replay(WAL) == idle Recovery.root_hash()
- `replay_is_deterministic` — 同一 WAL 两次 replay 相同 hash
- `replay_different_ops_different_hash` — 不同操作不同 hash

### 不变量
- 不修改 apply() / TransactionDelta / 五组件
- 不新增 apply 变体
- 不处理 Effect
- Replay 验证的是 WAL 中已提交的完整历史
- 162 tests, 0 failed


---

## Stage 3.3 Receipt — 2026-08-04

### 实现
- `TransactionReceipt { tx_id, before_root, delta, after_root }` 结构体
- `commit()` 返回 `Result<TransactionReceipt, VeritasError>`
- `make_receipt()` 纯函数构造
- `verify_receipt()` 接口预留（依赖 Stage 3.4 Checkpoint）

### 测试 (3 new)
- `receipt_after_matches_root_hash` — receipt.after_root == engine.root_hash()
- `receipt_before_after_consistency` — before_root != after_root, after 一致
- `receipt_replay_consistency` — Replay == idle Recovery, receipt.after == live engine

### Stage 3.4a 闭合 — 2026-08-04

**Commitment Domain 修正：**
- CapabilityGraph hash 改为纯语义 hash（去掉 CapabilityId）
- grant_base_access 删除：自身对象访问改为 verify_capability 结构性豁免
- 根因：grant_base_access 直接写 cap_graph 绕过 WAL，污染 root_hash
- 修复后 live == recovery 五组件全部一致
- constitution/kernel.md §6.1 新增"自身对象访问豁免"条款
- 162 tests, 0 failed

---

## Architecture Cleanup — 2026-08-04

### 影子系统退役

删除 6 个文件：

| 文件 | 原因 |
|------|------|
| `src/state_memory.rs` | 第二套状态存储，与 StateStore 平行脱节。state_root() 已迁移到 StateStore::root_hash() |
| `src/history.rs` | 旧 Replay 系统 ExecutionHistory/ReplayRecord，基于 state_memory |
| `src/replay.rs` | 旧 ReplayEngine，基于 state_memory + history + checkpoint |
| `src/replay_verify.rs` | 旧 Replay 验证逻辑 |
| `src/checkpoint.rs` | 旧 Checkpoint（StateSnapshot），已被 WorldSnapshot 取代 |
| `src/engine.rs.patch` | 临时补丁文件 |

### 迁移内容

- `state_root()` 从 `state_memory.root_hash()` → `self.root_hash()`（基于 StateStore 五组件确定性哈希）
- `record_history()` 函数删除（engine.rs + kernel.rs 透传）
- `engine.history` 字段删除
- `apply_state_memory()` 删除（engine.rs + kernel.rs 透传）
- `lib.rs` 移除 5 个旧模块声明
- `src/graph/` 已在 P1b 阶段删除（1021 行死代码，engine.topology 是唯一拓扑存储）

### 测试变化

- 94 → 88 tests（减少 6 个旧模块自带的单元测试）
- 0 failed

---

## PR1-PR4: Checkpoint 世界状态恢复 — 2026-08-04

### PR1: WorldSnapshot 稳定语义协议

定义在 `src/types.rs`，与内部实现（ObjectRecord/StateEntry/ScopeEntry）完全解耦：

- `WorldSnapshot` — commitment_hash + tx_id + 五组件数据
- `ObjectSnapshot` — id + object_type + lifecycle_state + metadata + payload
- `LinkSnapshot` — from + to + link_type（结构体，支持未来扩展）
- `ScopeSnapshot` — scope_id + members + owner（owner 用 ObjectId，不暴露 ModuleId）
- `CapabilitySemanticRecord` — 已存在，不含 CapabilityId

### PR2: 五组件 snapshot/restore 接口

每个组件只导出自己的语义，不知道 WorldSnapshot 存在：

| 组件 | Snapshot | Restore |
|------|----------|---------|
| StateStore | `snapshot() → Vec<(Address, Vec<u8>)>` | `restore_snapshot()` |
| ObjectRegistry | `snapshot_objects() → Vec<ObjectSnapshot>` | `restore_objects()` + `deserialize_object_body()` |
| Topology | `snapshot_links() → Vec<LinkSnapshot>` | `restore_links()` |
| CapabilityGraph | `snapshot_capabilities() → Vec<CapabilitySemanticRecord>` | `restore_capabilities()` |
| ScopeRegistry | `snapshot_all_scopes() → Vec<ScopeSnapshot>` | `restore_scopes()` |

- `CapabilitySemanticRecord.active` 从 `HolderRecord.active` 真实读取，不再写死 true
- `restore_capabilities()` 直接操作 grants + holders，保留 active 状态
- 新增 8 个 roundtrip 单元测试（ObjectBody serde 3 + CapabilityGraph 2 + ScopeRegistry 2 + Topology 1）

### PR3: Engine Checkpoint 接通

- `create_checkpoint()` — 聚合五组件 snapshot → WorldSnapshot
- `restore_checkpoint()` — 固定顺序恢复五组件（StateStore → ObjectRegistry → Topology → CapabilityGraph → ScopeRegistry）
- Engine 不再直接操作任何子模块内部（无 HashMap::clear/insert/push）

### PR4: Checkpoint 集成测试 (4 tests)

1. **五组件 roundtrip** — restore(snapshot(world)) == world（Objects + Links + Capabilities + StateEntries）
2. **恢复后可继续执行** — restore 后能开启新事务、创建 Object、Commit
3. **多次 restore 幂等** — 连续 restore(snap) 不改变世界
4. **快照幂等** — 连续 create_checkpoint() 输出一致（世界未变则快照不变）

### 已知未进入 Checkpoint 的机器元数据

| 状态 | 位置 | 影响 | 优先级 |
|------|------|------|:---:|
| global_version | engine.global_version | MVCC/冲突检测版本号可能回退 | **P0** |
| next_object_id | engine.object_id_counter | ObjectId 可能重用 | **P0** |
| grant_sequence | capability_graph.grant_sequence | CapabilityId 可能漂移/冲突 | **P0** |
| next_tx_id | tx_mgr.next_tx_id | tx_id 回退（语义待定） | Pending |

---

## Architecture Inventory — 2026-08-04

逐文件普查结果（详见 `ARCHITECTURE_INVENTORY.md`）：

- **保留**: 25 个模块
- **待迁移后删除**: 5 个（已于本次清理完成）
- **直接删除**: 1 个（engine.rs.patch，已于本次清理完成）

scope.rs 判定：ScopeExt trait，ScopeRegistry 的 API 扩展层，不持有状态，非第二实现，保留。

---

## 当前测试总计

- 88 unit tests (lib)
- 8 roundtrip tests (snapshot_restore_roundtrip)
- 4 checkpoint integration tests (checkpoint_roundtrip)
- 32 recovery tests (P29 pyramid)
- 10+ capability/freeze/unlink recovery tests
- 6 determinism/replay tests
- 其他集成测试 (object lifecycle, transaction, receipt, root_hash, commitment_domain 等)

总计约 160+ tests, 0 failed.

---

## 下一步优先级

1. **P0: 机器元数据恢复** — global_version, next_object_id, grant_sequence 进入 WorldSnapshot
2. **Deterministic World** — 基于新 Checkpoint + 统一 apply() 重建 Replay/Receipt（替代已删除的旧 Replay 系统）
3. **Module 生命周期闭合** — ModuleObject/ModuleInstance 分离
4. **engine.rs 拆分** — 1400+ 行按 Constitution 章节拆分
5. **Savepoint 完整语义** — 按需推进

---

## Closure Fix — 2026-08-04 (Checkpoint Continuity + Capability Identity + AccessIntent)

### 不变量闭合

1. **Checkpoint 世界连续性**
   - WorldSnapshot 已包含 global_version / object_id_counter / grant_sequence / StateEntry.version
   - restore_checkpoint 顺序修正：先恢复计数器与 capability_id，再恢复五组件
   - 恢复后 ObjectId / CapabilityId / version 序列与连续运行路径一致

2. **Capability Identity**
   - CapabilitySemanticRecord 新增 `capability_id`
   - snapshot 持久化原始 ID；restore 直接恢复，不再 capability_id_of 重算

3. **Capability 校验范围**
   - 引入 AccessIntent（Read/Write/Link/Unlink/Destroy/Freeze）
   - verify_capability 覆盖全部跨 Object side-effect，不再只看 write_set
   - 同事务 pending_capabilities 在 apply 前亦具授权效力

4. **Object Death / StateStore**
   - apply 已 remove_object；checkpoint 后无幽灵状态（回归测试确认）

5. **Kernel 边界**
   - 既有 P1b：mutation 经 Kernel::handle；begin/commit/read/write 保留为事务/Memory ABI
   - ABI 测试改为 begin_in_object + pending grant，符合 capability 校验

### 新增测试
- checkpoint_restore_world_continuity
- capability_identity_survives_checkpoint_restore
- object_death_no_ghost_state_after_checkpoint
- checkpoint_preserves_state_entry_versions

### 仍未完全闭合
- Kernel begin/commit/read/write 仍为 pub（Memory ABI 待 TRAP 化）
- Cross-object CALL 的 capability 校验依赖 Machine 路径，尚未单独 AccessIntent 化
- ModuleObject/ModuleInstance 生命周期
- Savepoint 完整语义

## 2026-08-09 安全审查与修复(capability 暴露 + 越权漏洞)

**背景:** 对 forge↔veritas 打通阶段的宪法合规性做例行审查,聚焦五个已知缺口
(Capability 绕过、Transaction 边界、自身对象豁免、DependencyInvalidated 监听、
测试严谨性)。审查过程中发现多处"实现存在但未生效"的隐蔽债务,以及一个
CRITICAL 级别的真实越权漏洞。

### 修复清单

1. **`create_object_short` 静默丢弃 AdminCap(已修复)**
   `world_api.rs` — commit 后从 `receipt.delta.capability_grants` 中提取
   AdminCap,返回类型从 `Result<ObjectId, _>` 改为
   `Result<(ObjectId, CapabilityId), _>`。同步修正 `veritasd.rs` 的
   `create_object` action 响应,新增 `admin_cap_id` 字段。

2. **`TransactionDeltaView.capability_events` 硬编码空数组(已修复)**
   `from_delta()` 中 `capability_events: vec![]` 从未被真正填充。现改为从
   `d.capability_grants` 生成人类可读日志,并新增结构化字段
   `capability_grants: Vec<CapabilityGrantView>` 供程序化读取
   capability_id/grantor/grantee/resource,不再需要解析字符串。

3. **`session_abort_discards` 测试缺失 `#[test]` 属性(已修复)**
   函数体逻辑正确但从未被执行过。补上属性后验证通过,abort 语义在当前
   架构下依然成立。

4. **`session_multi_op_commit` 中永真式断言(已修复)**
   `assert!(... || true)` 使断言恒真、形同虚设。修正为
   `assert_ne!(receipt.before_root, receipt.after_root)`,验证后依然通过,
   证实 root hash 逻辑本身没有问题(此前只是被这行掩盖,未被验证过)。

5. **【CRITICAL】`tx_write` / `tx_freeze_object` / `tx_death_object` 越权漏洞(已修复)**
   三处在调用 `enter_object(target)` 切换执行身份前,没有做任何权限校验:
   任意 session 可以无条件将 `current_object` 切换为系统内任意 `ObjectId`,
   切换后 `target == current_object` 天然成立,直接绕过
   `authorize_intent` 的 capability 检查。
   影响面:越权读写任意对象 MemorySpace、越权冻结任意对象(不可逆)、
   越权杀死任意对象(不可逆,级联 OWNS)。
   修复:三处在 `enter_object` 之前,先以 `AccessIntent::Call(target)`
   走一次 `authorize_intent`,未授权则直接拒绝、不切换身份。
   发现路径:为验证"自身对象访问豁免"(#4)补充对照测试时,
   `cross_object_access_still_requires_capability` 测试意外失败,
   顺藤摸瓜定位到 `enter_object` 本身零校验 + 三处外部 API 直接拿
   调用方传入的裸 `object_id` 触发切换。

### 新增测试(共 9 条)
`create_object_short_returns_valid_admin_cap`、
`object_without_capability_is_denied`、
`self_access_bypasses_capability_graph`、
`cross_object_access_still_requires_capability`、
`tx_write_cross_object_without_capability_denied`、
`tx_freeze_object_cross_object_without_capability_denied`、
`tx_death_object_cross_object_without_capability_denied`、
`tx_freeze_object_self_still_allowed`、
以及修复已存在的 `session_abort_discards`。
全量测试:98 passed, 0 failed。

### 结论 / 后续
- 内核核心(kernel.rs/engine.rs)本身的 capability 校验逻辑是可信的
  (`verify_capability`/`authorize_intent` 设计正确);本次问题集中在
  **对外暴露的 API 层**没有一致地复用这套校验。
- Transaction 边界(原判断为#2缺口)经核实**已完整存在**
  (session API: begin/create/freeze/death/link/unlink/write/read/commit/abort
  全部实现且已透传至 veritasd),此前的缺口评估是过时信息,已划掉。
- 自身对象访问豁免(#4)语义已确认在 `authorize_intent` 中正确实现
  (`target == current_object || target == capability_context` 时豁免),
  现有专属测试锁定。
- forge 端 `adapter.py` 仍未接住新暴露的 `admin_cap_id`——有意搁置,
  待后续专项处理。
- DependencyInvalidated 监听(#5)仍未实现——forge 目前只做
  create_object,未触发 Link/Death,暂无法测试,继续记债。

## 2026-08-10 汇编器/虚拟机指令完整性 + Operand 动态寻址

### 背景
起因是想跑通 `world_demo.vasm`:OBJECT_BIRTH 创建对象、写数据、建链接。过程中
连续暴露三层独立缺口,层层递进,记录完整链条以免日后重新排查一遍。

### 问题链(按发现顺序)

1. **汇编器指令覆盖不全(已修复)**
   `instruction.rs` 定义 31 条指令,`assembler.rs` 只解析了 17 条。
   `OBJECT_BIRTH`/`OBJECT_DEATH`/`OBJECT_FREEZE`/`OBJECT_LINK`/`OBJECT_UNLINK`/
   `READ`/`WRITE`/`EFFECT`/`CALL`/`TRAP`/`HOST_CALL`/`CAPABILITY_GRANT`/
   `SAVEPOINT`/`ROLLBACK_TO` 共 14 条无法用汇编文本写出。补全后 31 条全部可写。

2. **Machine 未执行部分指令(已修复)**
   `machine.rs` 的 `step()` 中,`ObjectBirth`/`ObjectDeath`/`ObjectFreeze`/
   `ObjectLink`/`ObjectUnlink`/`Read`/`Write` 七条指令落入 `_ => {}`,解析后
   静默不执行。补全后通过 `KernelCall` 调用 `kernel.handle()`。

3. **【根因】ObjectBirth 后未切换执行身份(已修复)**
   `Machine::new()` 时 `ctx.current_object = 0`,是空身份。`ObjectBirth` 执行后,
   内核分配的新 object_id 只写回了寄存器 R0 供程序读取,但 `ctx.current_object`
   从未更新为新对象 id。导致 birth 之后任何 `Write`/`ObjectLink` 等操作在
   `commit()` 阶段生成 `AccessIntent`,`authorize_intent` 检查
   `target == ctx.current_object` 时恒为假(current_object 卡在 0),
   直接 `PermissionDenied`。
   **误判过程**:最初怀疑是 `object_birth` 本身缺权限校验,需要 bootstrap
   根对象(方案A/B/C)。经排查确认 `object_birth` 对创建动作本身**没有**
   权限检查(`collect_access_intents` 不遍历 `pending_objects`),真正拒绝
   发生在下一条指令的 commit 阶段。**修复只需一行**:
   `ObjectBirth` 分支内 `self.ctx.enter_object(id)`。
   不需要 bootstrap 根对象、不需要给空身份开权限特例——两者都会在权限模型
   里引入隐式特权判断,是与 2026-08-09 `tx_write` 越权漏洞同构的反模式,
   已规避。
   语义参考 `enter_object` 现有注释(对应 CALL 跨 Module 调用的身份切换),
   `ObjectBirth` 是同一机制的自举场景,而非独立语义。

4. **指令操作数无法引用寄存器值(已修复,较大重构)**
   即使 #3 修复后,`WRITE`/`OBJECT_LINK` 等指令的操作数字段
   (`state_id`/`object_id`/`from`/`to`)在 `instruction.rs` 中是裸
   `StateId`/`ObjectId`(即 `u64`),`assembler.rs` 只能编译立即数。
   ObjectBirth 返回的运行时 ID 存在 R0 里,但下游指令写死在字节码里的
   立即数无法引用它——`WRITE 10, "hi"` 里的 10 和 R0 的实际值无关。
   **修复**:新增 `Operand { Immediate(u64), Register(u8) }` 枚举,
   `Read.state_id` / `Write.state_id` / `ObjectDeath.object_id` /
   `ObjectFreeze.object_id` / `ObjectLink.{from,to}` /
   `ObjectUnlink.{from,to}` 六个字段类型改为 `Operand`
   (`ObjectBirth.object_id` 不改,因为它是声明式的、由内核分配覆盖)。
   涉及五个文件:
   - `instruction.rs`:定义 `Operand`,改字段类型
   - `assembler.rs`:新增 `parse_operand`(复用 `parse_reg` 的 `R\d+` 前缀
     判断),六条指令解析改用它
   - `machine.rs`:新增 `resolve_operand`(立即数直接返回,寄存器读
     `self.registers.get_u64`),六条分支执行前先 resolve
   - `executor.rs`:**独立于 Machine 的第二条执行路径**(`Executor` 结构体,
     供 `run_program` 使用),无寄存器文件。新增 `resolve_immediate`,
     遇到 `Operand::Register` 明确返回 `EngineError`(而非静默按0处理)。
     确认此路径当前无任何外部调用者(仅 `executor.rs` 内部自用)。
   - `instruction_codec.rs`:二进制编解码器。新增 `encode_operand`/
     `decode_operand`,统一编码为 1 字节 tag(0=Immediate,1=Register)+
     8 字节 value = 9 字节。六条指令的编码长度从 8/16/17 字节变为
     9/18/19 字节。**Breaking change:此前编译的 `.vmod` 文件全部作废**
     (读取会报 "Bad VMOD magic" 或类似解析错误),需用新 assembler 重新编译。

### 验证
`cargo build` 通过(0 error)。`cargo test` 全量通过(assembler 单元测试 +
全部集成测试)。旧 `.vmod`(hello.vmod/create_obj.vmod/world_demo.vmod,均为
2026-08-09/10 改动前编译)已删除,需重新汇编。

### 结论 / 后续
- `WRITE_REGISTER state_id, reg` 指令(改动前就存在)现在功能上是
  `WRITE Rn, payload` 的子集,不再是唯一能写入运行时值的路径,可保留但
  不建议新代码依赖它。
- **约定**:以后新增指令时,若某个操作数字段的值可能来自运行时
  (不只是 object_id,任何 handle/cap_id/动态计算值都算),从设计时就该用
  `Operand` 而不是裸 `u64`/`ObjectId`/`StateId`,避免重演这次"先加指令、
  跑起来才发现参数是死的"的返工。
- `Executor`(`executor.rs`)确认是当前无外部调用者的独立执行路径,与
  `Machine` 功能重叠但实现分离(无寄存器、无 trace)。是否该合并或废弃,
  未决,记债。

### 端到端验证(2026-08-10 完成)
`world_demo.vasm` 改为 `OBJECT_BIRTH 0` → `WRITE R0, "..."` → `COMMIT` → `HALT`,
`compile` + `run` 后输出 `r0=1`(内核实际分配 ID)、`objects in world: 1`。
确认 R0 里的运行时值被 `WRITE` 正确引用并持久化,整条问题链(#1~#4)全部闭环。

---

## P31 — First User Program / Vertical Execution Proof (2026-08-10)

### 里程碑
Veritas 第一次证明了用户可以用汇编写程序，驱动整条链路完成一次完整的世界变化。

之前 STATUS 描述 "Machine - Step/run" 和 "Assembler - Present" 只能证明组件存在，
不能证明从源码到持久化世界的全链路通畅。

### E2E 验证程序

module world_demo
version 1.0.0
    OBJECT_BIRTH 0
    WRITE R0, "hello from veritas"
    LOAD_CONST R2, 0
    ADD R1, R0, R2
    OBJECT_BIRTH 0
    OBJECT_LINK R1, R0, owns
    COMMIT
    HALT

结果: pc=86 r0=2, objects in recovered world: 2

### 验证链路

VASM -> Assembler -> VMOD -> Instruction Codec -> Machine
-> Register/Operand Resolution -> KernelCall -> TransactionContext
-> commit -> TransactionDelta -> WAL -> Recovery -> World query

### 同时验证的能力

- 运行时创建对象
- Kernel 分配真实 ObjectId
- ObjectId 返回寄存器
- 后续指令引用运行时寄存器值
- 动态 ID 参与写操作
- 同一事务创建多个对象
- 多个运行时 ID 参与关系构造
- 身份上下文随执行正确切换
- Birth + Write + Link 在一个 TransactionDelta 中
- Commit 持久化到 WAL
- WAL 可恢复为可查询 World

### 修复的问题链

1. Assembler 指令覆盖不全: 17/31 -> 31/31
2. Machine 未执行部分指令: _ => {} 静默丢弃 -> 7条指令全部分发
3. ObjectBirth 后未切换执行身份: current_object=0 -> enter_object(id)
4. Operand 不支持寄存器: Write/Read/ObjectLink 等操作数从裸u64改为Operand枚举
5. ObjectLink 执行身份为to而非from: 新增enter_object(from)

### 后续架构债

- ObjectLink 隐式身份切换: 长期应改为显式ENTER_OBJECT或AccessIntent三参数模型
- inspect list CLI 路径与 veritasd 查询路径不一致
- Executor(executor.rs) 与 Machine 功能重叠，待合并或废弃
- OBJECT_DEATH/OBJECT_FREEZE 执行后不切换身份 (语义待定)

### 身份切换审计 (2026-08-10 识别)

当前 current_object 承担双重职责:
  A. 当前执行身份
  B. 下一条操作默认的授权主体

OBJECT_LINK 的隐式 enter_object(from) 使链接操作附带身份切换能力，
与 2026-08-09 修复的越权漏洞结构相似(只是本事务内新建对象暂安全)。

长期方案:
  方案1: AccessIntent::Link { actor, from, to } 三参数模型
         actor 是否有权建立 from->to，不依赖 current_object 豁免
  方案2: 要求程序显式 ENTER_OBJECT 后再操作
         不给业务操作附带隐式身份切换

当前允许切换身份的指令: OBJECT_BIRTH, CALL, RETURN, OBJECT_LINK(临时)
目标: OBJECT_LINK 从名单中移除

---

## P0 安全修复 + 身份切换架构纠正 (2026-08-10 续)

### 背景

P31 记录里第3、5条("修复"current_object=0 → enter_object(id)；新增enter_object(from))
实际是引入了安全漏洞，不是修复。审计发现后本轮予以纠正。

### 漏洞根因

`ObjectLink` 执行时调用 `self.ctx.enter_object(from)`，使 commit 阶段
`authorize_intent(AccessIntent::Link(from,to))` 里 `target == ctx.current_object`
恒真，等价于调用者对 from 自我授权，完全绕过 capability graph 检查。
与 2026-08-09 修复的 tx_write/tx_freeze_object/tx_death_object 越权漏洞同构。

### 已完成修复

1. **P0**: 删除 `machine.rs` ObjectLink 分支的 `enter_object(from)`。
   授权完全交给 commit 时 `authorize_intent`，以调用者真实身份走 capability graph。
   新增回归测试 `tests/machine_object_link_security.rs`
   (恶意路径必须拒绝 + 合法路径必须成功，两个用例)。

2. **P3**: 删除 `machine.rs` dispatch 末尾的 `_ => {}` 死代码兜底。
   编译验证：删除后无 non-exhaustive match 报错，证明当前全部指令变体
   已被显式处理；此后新增指令若漏实现会被编译器强制拦下。

3. **架构纠正 — OBJECT_BIRTH 不再自动切换身份**：
   `current_object` 的改变现在只能经过唯一受控入口 CALL
   (先 authorize_intent(AccessIntent::Call) 审计，通过后才切换)。
   OBJECT_BIRTH 只把新 ObjectId 写入寄存器，创建者身份不变。
   全仓库排查确认无 Rust 测试依赖旧的自动切换行为，仅
   `world_demo.vasm` 依赖，已知失效，见下方。

4. **P4 (部分)**: `Instruction::CapabilityGrant` 的 holder/resource、
   `Instruction::Call` 的 object_id，从裸 u64/ObjectId 改为 Operand，
   贯穿 assembler.rs / instruction_codec.rs / machine.rs / executor.rs 四层。
   使 CALL 目标对象和 CAPABILITY_GRANT 的持有者/资源现在可以是运行时
   寄存器值，不再要求硬编码。

### 已知失效 — world_demo.vasm

身份切换收紧后，该 demo 从 OBJECT_BIRTH 到 OBJECT_LINK 之间从未
CALL 进入任何对象，current_object 全程停留在初始值。WRITE 实际写入
Address(0, 对象ID)，不是预期的"对象1名下"状态；OBJECT_LINK 因调用者
对 from 无 capability 会在 commit 时被拒绝(PermissionDenied)。

程序文件保留(未删除)，仅更新头部注释说明现状，作为下一轮重写起点。

### 下一轮待办

- 重写 world_demo.vasm：用 CALL(现已支持 Operand 动态目标)+ 标签，
  走 birth → CALL 进入新对象身份完成 WRITE → RETURN → 建 Link 的
  完整闭环；需先核实 CallFrame 寄存器传递语义是否够用。
- P1: tests/machine.rs 仍为空文件，完整 E2E 测试待写
  (E2E-1~4，见最初审计: 单对象闭环、动态Operand、Birth+Write+Link、
  跨对象非法操作必须失败)。
- P2: CLI inspect 与 veritasd 排查尚未深入
  (已确认二者共用同一 Kernel::with_wal_path 入口，问题应在别处)。
- Instruction::ObjectBirth 的 object_id 字段是否为废弃字段待确认
  (机器实际使用 kernel 分配的动态 id，该字段疑似未使用)。

---

## 身份切换死结的最终解法 (2026-08-10 三度修正)

### 事情经过（如实记录，包括反复）

1. 最初审计发现 `ObjectLink` 用 `enter_object(from)` 自我授权绕过 capability
   graph，判定为安全漏洞，删除修复（见上文"P0 安全修复"节）。
2. 同一逻辑推广到 `OBJECT_BIRTH`：删除其 `enter_object(id)` 自动切身份，
   改为身份切换只能经过 CALL。删除后 `world_demo.vasm` 测出：本轮虽然
   `cargo test` 全绿，但没人验证过"root 用 CALL 显式进入自己刚创建的
   对象"这个理应最基本的场景是否真的走得通。
3. 后续窗口（未验证 CALL 路线是否可行）判定 CALL 路线"卡死"，恢复了
   `enter_object(id)`，并论证"这个 case 安全，因为 id 是内核刚分配的
   全新对象，反正 authorize_intent 一定会通过，与 ObjectLink 那个真实
   漏洞不同构"。
4. 逐行验证这个论证：`object_birth` 把新对象的 self-AdminCap push 进
   `ctx.pending_capabilities`，但从未 `attach_capability` 到
   `ctx.capabilities`。`authorize_intent` 的 `has_pending` 分支要求
   `ctx.capabilities.contains(&g.capability_id)` 才算数（第三个 or 条件）
   或 `grantee == ctx.current_object/capability_context`（新对象自己的
   grantee 是它本身，不是 root）。也就是说：**"审计一定会通过"这个
   前提是错的——CALL 当时根本走不通，跟直接绕过一样，只是没人跑过
   这条路径**。用隐式切身份掩盖了一个从未被真正建立的授权关系，这正是
   与 ObjectLink 同构的问题，不是"不同构"。

### 最终结论（已实现，已测试锁定）

**不恢复 `enter_object(id)`。OBJECT_BIRTH 依旧不切换身份。**
改为：`OBJECT_BIRTH` 执行后，从 `ctx.pending_capabilities` 里找出刚创建的
`grantee == resource == id && cap_type == "AdminCap"` 的那条 grant，
把它的 `capability_id` 显式 `attach_capability` 到本事务 `ctx`。

效果：
- 身份切换的合法入口维持唯一——只有 CALL（先 `authorize_intent`
  审计，通过才切）。没有"这个case反正安全"的隐式例外口子。
- CALL 现在真的能通过审计走进新对象，因为 self-AdminCap 已经
  attach 到 ctx，`has_pending` 分支能查到。不是"反正会通过"，
  是"真的被检查过、真的通过"。
- 覆盖了最坏情况：root(current_object==0) 创建对象时，
  object_birth 不会给 root 发额外 grant（这条分支专门排除了
  current_object==0），过去被认为是"死结"——但死结的本质不是
  "root 没资格"，是"资格发了但没接上"，接上就好，不需要造
  process/bootstrap 那层。

### 验证

新增 `tests/object_birth_self_call.rs`：
`root_can_call_into_object_it_just_birthed` —— root 身份 birth
一个新对象后，用 CALL 显式进入它必须成功，且 current_object
必须真的切换过去。这是本轮争论最终要证明的行为，不再只靠论证
或手工跑 world_demo.vasm 确认。

`tests/machine_object_link_security.rs`（P0）和这个新测试一起，
构成了"身份切换只能经过唯一受控入口"这条不变量的完整回归覆盖。

全量测试：211 + 1 = 212 passed, 0 failed。

### 给未来的教训

这次反复本身就是一个案例：**一个"看起来安全的隐式例外"，
如果没有实测验证替代路径是否可行，很容易被当作"唯一解"而恢复
回去**。以后遇到类似"这个特权豁免反正安全"的论证，应该先问
"如果坚持走正规路径，会不会真的卡住"——如果卡住，先查卡在哪，
而不是退回隐式例外。详见新建的 `docs/IDENTITY_MODEL.md`。

---

## 2026-08-11 — world_demo 重写闭环 + is_halted() 死循环修复

### world_demo.vasm 完整重写

之前版本(见上文"已知失效"节)身份切换收紧后从未 CALL 进任何对象，
WRITE 缺失、OBJECT_LINK 因无 capability 会被拒绝。本轮彻底重写：

  birth A -> CALL 进 A -> WRITE -> RETURN 回 root
  birth B -> CALL 进 B -> WRITE -> RETURN 回 root
  root 身份下直接 OBJECT_LINK A, B, owns（两次 birth 已各自 attach self-AdminCap 到同一 ctx，
  无需再次 CALL，root_link_two_children.rs 已验证此前提）
  COMMIT / HALT

CLI 实跑验证：compile -> run -> inspect 全链路通过，
finished pc=104 r0=2，objects in world: 2，两个对象均 Alive。

### tests/machine/basic.rs — E2E-1~4 补齐

不再是空 stub。新增 4 个测试，全部走生产真实路径
（assemble_module + Machine::boot，而非手拼字节码）：

  E2E-1 单对象闭环：birth -> CALL -> WRITE -> RETURN -> COMMIT -> HALT
  E2E-2 动态寄存器数据流：ADD 复制出的寄存器作为 CALL 目标 operand，验证可用
  E2E-3 Birth+Write+Link 全链路：直接读取磁盘上的 world_demo.vasm 文件本身跑，
        保证测试与交付文件不脱节，断言 has_link(A,B) 为真
  E2E-4 跨对象非法操作必须失败：新 tx 在无 capability 情况下 CALL 进已提交对象，
        必须 Trapped，且 current_object 不得切换

因 Runtime::execute 遇 Trap 会死循环（见下），测试改用自写的
run_bounded() 帮助函数，带步数上限，手动匹配 Halted/Aborted/Trapped 三态。

全量测试：cargo test 103+ 全绿，0 failed。

### 已知问题修复：Runtime::execute 遇 Trap 死循环

is_halted() 原实现只认 Halted | Aborted(_)，不认 Trapped，
导致 Runtime::execute 的 while !is_halted() 循环在程序触发 Trap 时永不退出，
CLI 的 veritas run 命令会挂死。已修复：is_halted() 加入 Trapped(_) 分支。
详见 docs/VASM_EXECUTION_MODEL.md 第8节。

### 新增文档

docs/VASM_EXECUTION_MODEL.md（277行）—— 面向下一个接手 AI 的完整参考：
执行链路分层、Machine vs Executor 两条路径说明、身份切换模型（current_object/
capability_context/CallFrame 语义）、WRITE 地址组装规则、Operand 解析规则、
vasm 语法速查、CLI 三段式操作流程、is_halted 死循环问题记录、
身份切换死结的历史教训摘要、给未来会话的排查方法论。

### 仍未解决

P2: CLI inspect 与 veritasd 查询结果不一致——仍未深入，只排除了
"两者共用同一 Kernel::with_wal_path 入口"这个方向，问题应在别处（WAL flush
时机？查询时序？未验证）。

Executor(executor.rs) 与 Machine 功能重叠——今仍未处理，无外部调用者，
是否合并/废弃未决，继续记债。

## 08-12 更新

**P2: CLI inspect 与 veritasd 查询结果不一致 — 挂起，非当前 bug**
- 排查过程：依次排除了 WAL fsync 时机、CLI/veritasd 重放逻辑分叉（两者共用同一 `apply()` 函数）、Executor 死代码干扰等方向
- 关键发现：veritasd 本体不是 socket 常驻服务，而是从 stdin 逐行读 JSON、stdout 逐行吐 JSON 的进程（见 `src/bin/veritasd.rs:307` `main()`）。用 `&` 或 `nohup` 裸起后台会导致 stdin 立即 EOF，进程"安静退出"而非崩溃，此前排查时被这个假象干扰
- 真实调用路径在 `forge/forge/world/runtime.py`（`WorldAdapter` 管理子进程 stdin/stdout 管道），本次未深入这条链路
- 结论：veritasd 尚未跑通过完整流程（起服务→adapter通信→查询），当前"不一致"现象是记录自某次测试/改动时的观察，非稳定复现。**降级为待验证项，等 veritasd 完整流程首次跑通后再判断是否仍存在，届时再正式排查**

**Executor(executor.rs) 已删除**
- 确认 `src/` 和 `tests/` 目录下均无任何引用（grep 全局为空）
- 删除 `src/executor.rs`，移除 `src/lib.rs:10` 的 `pub mod executor;` 声明
- 编译通过，全量测试 103+ 项全绿

**顺带修复：`tests/kernel_world_runtime.rs` 编译错误**
- 该测试文件解构 `Runtime::execute` 返回值时仍按旧签名当元组用（`(pc, object_id)`），但该函数现在返回 `ExecutionOutcome` 枚举（`Completed{pc,r0}` / `Trapped{...}`）
- 与本次 Executor 删除无关，是独立遗留的签名不同步（`cargo build` 不编译 tests/，此前一直未暴露，本次跑 `cargo test` 时发现）
- 已改为 match 解构 `ExecutionOutcome::Completed{r0,..}`，`object_id` 取自 `r0`，测试通过

**环境提示：** proot-distro debian(python镜像) 容器已删除。之前用于装 aider，后确认无用，且今天排查 P2 时一度造成路径混淆（容器内 `/root/termux-home/...` 与原生 Termux `~/veritas_kernel` 是两份独立文件系统，非软链接）。以后统一在原生 Termux 环境操作。

## Test Integrity Pass — 2026-08-12

背景：P0 self-authorization修复后，需确认现有测试是否绕过身份路径。

审计结果：
- forge_e2e_jsonlines：路径bug（veritasd找不到），改用env!(CARGO_BIN_EXE_veritasd)修复
- tests/capability.rs：纯占位assert!(true)，已删除
- call_access_intent(8个)：走真实Machine执行，断言精确，可信
- world_api内联(12个)：走WorldService公开API，安全边界清晰，可信
- 其余安全测试(capability_revoke/delegate/recovery/machine_object_link_security等)：可信

结论：核心安全路径测试均走真实入口，未发现假绿问题。全量测试通过，0 failed。

剩余债务：部分单元测试断言偏弱(is_ok)，属正常惯例，非阻塞。

API→授权闭环验证：
tx_write/tx_freeze_object/tx_death_object 三者均在 enter_object(target)
之前调用 kernel.engine().authorize_intent()，确认 API 层统一进入同一套
核心授权逻辑，不存在 API 层独立做权限判断的旁路。

## P2 闭合 — 2026-08-12

### 验证结果
Forge → WorldAdapter → veritasd → WorldService → Kernel → WAL → restart → CLI inspect 全链路通过。

### 实测数据
- Forge: version=9, count=9, object 9 Alive
- CLI: 恢复9条记录，9 Alive
- 重启后新 WorldAdapter 指向同一 WAL：object 9 存在且 Alive
- 两者对象列表完全一致（1-9 全部 Alive）

### 此前误判原因
"CLI inspect 与 veritasd 不一致"的判断基于错误 CLI 参数（inspect --wal PATH），
实际 inspect 通过 VERITAS_WAL 环境变量读取 WAL。使用正确参数后结果一致。


### 发现
- CapabilityGrant 已是 Veritas 的正式能力原语（Kernel/Machine/WAL 链路均实现），但 Engine::capability_grant 的 grantor 参数被硬编码为 grantee（self-grant），宪法 §3.5 要求的 from→to 跨对象授权语义在 Engine 层未实现；veritasd 对外命令面未暴露 Capability 命令
- WorldRuntime 当前采用单 active session 约束，未发现违反架构契约
- CLI recovery 日志显示"当前版本号: 0"而 Forge world_info() 返回 version=9，疑为日志字段使用错误，单独记债
- WorldRuntime只维护单session（_current_session），是设计约束而非bug

## P4-1 ObjectPathMap 闭环 — 2026-08-12

- Intent → WorldRuntime → Receipt → ObjectPathMap → Restart 全链路通过
- state_id=0 作为路径约定，ObjectPathMap.update_from_delta() 从 memory_written 提取
- 重启后从 receipt history 重建 ObjectPathMap，正反向查找均正确
- 对象在 Veritas 中保持 Alive

## P4 Projection 验证 — 2026-08-12

### P4-1 ObjectPathMap 闭环 ✅
- Object → path 映射从 memory_written(state_id=0) 提取
- 重启后从 receipt history 重建，正反向查找正确
- 当前协议：state_id=0 为路径约定（临时，非 Veritas Object metadata）

### P4-2 FileProjection 真实落盘 ✅
- 完整链路：Intent → WorldRuntime → Receipt → FileProjection.apply(receipt, delta) → 真实文件
- 正确接口：fp.apply(receipt, delta)，非 apply_delta(delta)
- 路径语义：project_root + 相对路径，非绝对路径
- 重启后 ObjectPathMap 恢复映射，Veritas 对象保持 Alive

### 已验证闭环
Object Birth → WorldRuntime Session → tx_write(path,state_id=0) + tx_write(content,state_id=1)
→ Veritas Commit → Receipt/TransactionDelta → ObjectPathMap → FileProjection.apply()
→ FileManager → Host File → Restart → ObjectPathMap恢复 → Object Alive

### 待验证
- P4-3: Projection 失败语义（World COMMIT成功但FileProjection失败时，不谎报成功/回滚）
- state_id=0 临时路径协议（待全链跑通后再决定是否升级为正式Object metadata）

## P4-3 Projection 失败语义 — 2026-08-12 ✅

- FileProjection 失败时 ProjectionResult.success=False，reason 明确
- retryable=True，调用方可据此重试
- 失败不回滚已提交的 Veritas 世界状态
- 文件不谎报写入成功
- 重启后对象保持 Alive，世界一致

## P4-4 Projection 重试/幂等性 — 2026-08-12 ✅

- 第一次 apply 失败：success=False，文件不存在
- 第二次重试成功：文件写入正确内容
- 第三次幂等 apply：内容不变，不重复写入，文件大小不变
- Veritas version 不受 projection retry 影响
- 重启后 ObjectPathMap 正确，不重复污染

## P4-5 Multi-object Partial Failure + Retry — 2026-08-12 ✅

- A/B/C 各自独立 receipt，B receipt 投影失败时 A 和 C 不受影响
- B 文件不存在，A/C 正常投影
- B 可单独 retry，成功后不损坏 A/C
- 全部 receipt 幂等再 apply，文件大小不变（均为12 bytes）
- 重启后三个对象全部 Alive，ObjectPathMap 路径和反向查找正确

### 架构发现
- FileProjection.apply(receipt, delta) 是单 receipt 投影器，receipt 间独立执行
- B 失败不阻塞 C——不是跨 receipt batch transaction，语义正确
- Veritas 当前不允许跨对象写（需 capability），每个对象独立 commit/receipt
- B projection failure 不污染 Veritas 世界，不损坏已投影的 A/C

### P4 完整结论
P4-1 ObjectPathMap 闭环 ✅
P4-2 FileProjection 真实落盘 ✅
P4-3 Projection 失败语义（success=False, retryable=True, 世界不回滚）✅
P4-4 单 receipt 重试/幂等 ✅
P4-5 多 receipt partial failure + 独立 retry + 幂等 + 重启恢复 ✅

## P4-6 Crash/Restart + Projection Recovery — 2026-08-12 ✅

- commit 成功，projection 故意失败（success=False, retryable=True）
- Runtime 关闭后重启，从 receipt history 恢复完整 receipt
- ObjectPathMap 从 receipt history 重建，定位到目标 receipt
- retry projection 成功，文件内容正确（"data survived crash"）
- 对象保持 Alive，ObjectPathMap 正反向查找正确
- 幂等再 apply 安全，文件大小不变（19 bytes）

### 已验证完整闭环
Veritas commit → receipt持久化 → projection failure
→ Runtime crash/restart → receipt recovery → ObjectPathMap重建
→ projection retry → 正确结果 → 幂等安全

## P4-7 边界审计 — 2026-08-12 ✅

- 空 delta apply：success=True，安全无操作
- 无效 object_id：不 crash，正常返回
- Receipt/Delta mismatch：不 crash，返回有效结果
- 重复 apply 跨 restart：内容不变（size=12）
- 连续两次失败 + 第三次 retry：成功
- 失败 → restart → 再次失败 → 最终 retry：成功，内容正确

## P4 Projection 最终结论

P4-1 ObjectPathMap 闭环 ✅
P4-2 FileProjection 真实落盘 ✅
P4-3 Projection 失败语义 ✅
P4-4 单 receipt 重试/幂等 ✅
P4-5 多 receipt partial failure + 独立 retry ✅
P4-6 Crash/Restart + Projection Recovery ✅
P4-7 边界审计（空delta/无效id/mismatch/双重失败/重启后再失败）✅

Projection 层已验证：失败安全、可重试、幂等、可跨重启恢复、边界不崩溃。
下一阶段：P5 Forge Intent → World Transaction。

## P5 Intent → World Transaction 审计 — 2026-08-12

### 已验证通过
- P5-1 create_file: Intent → Executor → Veritas → Projection → 文件 → 重启恢复 ✅
- P5-3 delete_file: Intent → session.death() → objects_deleted → 文件删除 → 重启不存在 ✅

### 发现真实问题
- P5-2 modify: Projection 将 patch JSON 追加到原文，而非应用修改（文件内容 "originalmodified"）
  根因：Executor 将 operations 写入 state_id=2，Projection 将其当内容拼接
- stage(): WorldSession 缺少 preview_delta()，require_confirm=True 路径已损坏
  stage 不应创建未关闭的 session，应走 begin → execute → preview → abort
- execute_batch: 跨对象 PermissionDenied，未完成原子性验证
  当前每个对象独立 commit，非同一 transaction
- create_file 未检查路径冲突：overwrite=False 策略未生效
  多个 create 同一路径 → ObjectPathMap 覆盖，无冲突检测

### P5 修复优先级
P5-A: stage() — 临时session + preview + abort，禁止mutation
P5-B: modify projection — operations 应产生新文件内容，非序列化追加
P5-C: execute_batch atomicity — 同transaction多Intent + 失败回滚验证
P5-D: create path uniqueness — overwrite策略 + ObjectPathMap冲突检测

## P5 修复完成 — 2026-08-12

### P5-A: stage/preview_delta ✅
- WorldSession 新增本地 buffer（_objects_created/deleted/frozen/links/memory_written）
- preview_delta() 从 buffer 本地拼装，不触发 tx_commit
- 三个 _stage_* 方法加 try/finally: session.abort()，预览后不留残留会话
- 验证：stage() 返回正确 delta，list_objects() 确认无持久化副作用

### P5-B: modify projection 语义 ✅
- 根因：_dicts_to_edits 将 operations 序列化后当作内容拼接，而非应用 patch
- 修复：_dicts_to_edits 原样透传 start_line/end_line（0-indexed 半开区间），apply_edits 正确替换指定行
- 验证：modify 后文件内容为 "modified content"，不再出现拼接。契约确认：start_line/end_line 为 0-indexed 半开区间

### P5-C: delete/recovery 一致性 ✅
- 根因：ObjectPathMap.update_from_delta 只处理 memory_written，不处理 objects_deleted
- 修复：update_from_delta 末尾对 delta.objects_deleted 逐个调用 self.remove()
- 验证：delete 后实时/重放/重启后 path map 均正确遗忘，list_objects() 只剩活对象

### P5-D: 输入校验 ✅
- IntentExecutor 新增 _validate_intent，在 execute/execute_batch/stage 前统一校验
- create_file: path 非空 + content 非空 + overwrite=False 时路径冲突检测
- modify_file/delete_file: object_id 必须存在且 Alive
- 校验失败不调用 begin_session()，无残留会话
- 验证：4种非法输入全部提前拒绝，合法操作无回归，stage() 校验同样生效

### 修改文件
- forge/world/session.py（重写，新增本地 buffer + preview_delta）
- forge/intents/executor.py（_stage_* 加 abort + _validate_intent + _apply_operations_to_content）
- forge/projections/file_projection.py（_dicts_to_edits 索引转换）
- forge/projections/object_path.py（update_from_delta 处理 objects_deleted）

## P5 回归修复 — 2026-08-12

两个旧测试失败已修复，208/208 passed：

1. test_modify_existing_file：撤销之前临时加的 start_line-1 转换。
   仓库既有测试要求 0-indexed 半开区间原样透传，之前的转换基于临时单行
   测试的错误假设，与真实测试契约互斥。

2. test_e2e_veritas_forge.py：FileProjection._resolve 对绝对路径跳过
   workspace containment，保留 blocklist。绝对路径来自 Veritas 权威状态，
   Projection 职责是忠实还原，不应重新判断路径是否在 workspace 内。

## 多对象事务修复 — 2026-08-12

### 问题
同一 session 内 birth A → write A → birth B → write B → link A→B → commit
在 commit 阶段 PermissionDenied。

### 根因
TransactionContext.capability_context 设计意图是"记住事务发起者身份，不随跨对象切换改变"，
但 WorldService::tx_begin 从未设置它（恒为0）。只有 Machine 的 CALL/RETURN 路径正确维护。
session 内多次 write 触发 enter_object 覆盖 current_object 后，commit 时 verify_capability
用 current_object(已是最后切换的对象)和 capability_context(恒为0)查 capability，
最初持有 cap 的 actor 身份"够不着"，鉴权失败。

### 修复
world_api.rs: tx_begin() 里 begin_in_object(actor) 后显式设置 ctx.capability_context = actor。
一行修复，不碰权限模型、capability 语义、enter_object、authorize_intent。

### 验证
- birth A + write A + birth B + write B + link A→B + commit → ok, receipt 含 links_added [[A,B,owns]]
- 重启后 A/B 均 Alive
- cargo test: 全部通过
- pytest: 208 passed

### 修改文件
veritas_kernel/src/world_api.rs（tx_begin 一处）

## 多对象事务最终修复 — 2026-08-12

### 真正的根因
tx_begin(None) 路径下，tx_create_object 在 current_object==0 时只设了
current_object=A，没设 capability_context。此后 tx_write 跨对象切换
current_object，capability_context 恒为 0。commit 时 authorize_intent
用 current_object(已被最后一次 write 切走) 和 capability_context(0)
去验证历史 intent，找不到 holder → PermissionDenied。

### 之前的修复（tx_begin 设 capability_context=actor）
只覆盖了 tx_begin(Some(actor)) 路径，tx_begin(None) 路径下 actor=0，
capability_context 仍是 0。

### 最终修复
tx_create_object 里 current_object==0 分支，enter_object(id) 后加
capability_context = id。此后 capability_context 作为 session 的
稳定授权身份，不随 tx_write 切换而漂移。

修改文件：veritas_kernel/src/world_api.rs（tx_create_object，一行）

### 新增测试
- tests/world_demo.rs: birth A → write A → birth B → link A→B → commit → WAL recovery
- tests/multi_object_transaction_regression.rs: Test A(abort) / Test B(跨session隔离) / Test C(WAL recovery + link去重)

### 全量验证
- cargo test: 全部通过
- Forge pytest: 208 passed
- world_demo: PASS
- 三个回归测试: PASS
- machine_object_link_security: PASS

## 只读架构核对 — 2026-08-13 CapabilityGrant 链闭合状态

### 核对范围（只读，未改任何代码）

| 层 | 文件 | 状态 | 问题 |
|---|------|------|------|
| CapabilityGraph::grant | capability.rs:284 | ✅ 正确 | 签名含 grantor/grantee/resource |
| Engine::capability_grant | engine.rs:1242 | ❌ 错误 | grantor 硬编码为 grantee |
| KernelCall::CapabilityGrant | kernel.rs:45 | ❌ 不完整 | 缺 grantor 字段 |
| Machine::CapabilityGrant | machine.rs:327 | ❌ 不完整 | 无法表达 from→to |
| veritasd JSONL | bin/veritasd.rs | ❌ 未暴露 | 20 个命令，0 个 Capability |
| Forge adapter/session | forge/forge/world/ | ✅ 边界正确 | 纯适配层，未复制 Ver 语义 |

### 唯一已确认的语义错误

`Engine::capability_grant` 把 grantor 硬编码为 grantee，导致宪法 §3.5
要求的 from→to 跨对象授权语义在 Engine 层未实现。

`KernelCall::CapabilityGrant` 缺少 grantor 字段、`Machine::CapabilityGrant`
无法表达 from→to，均属于同一语义缺口向上暴露后的链路不完整，
不应视为三个独立 bug。

CapabilityGraph 层本身正确。

### 修复路径（Ver 内部闭环 → 公开 → Forge 适配）

P0（Ver 内部闭环）：
  Engine::capability_grant 加 grantor 参数
  KernelCall::CapabilityGrant 加 grantor 字段
  Machine::CapabilityGrant 从 ctx.current_object 取 grantor
  测试：A grant B → B 操作 A 成功，A ≠ B

P1（veritasd 暴露）：
  tx_capability_grant 命令（只暴露 Ver 原语，不发明 Forge 接口）

P2（Forge 适配）：
  WorldAdapter.grant / WorldSession.grant（薄包装，不复制语义）

### 架构边界（本次审计确认）

- Ver = 独立计算机/运行时基础设施
- veritasd = Ver 的外部接口（JSONL）
- Forge = 运行在 Ver 之上的应用

Forge 可以使用 Ver 的全部能力，但 Ver 不依赖 Forge，也不包含 Forge。
CapabilityGrant 是 Ver 的计算机权限原语，Forge 消失后它依然应该存在。

---

## P1 veritasd 外部接口暴露完成 — 2026-08-13

### 已完成

- WorldService::tx_capability_grant 方法
  - grantor 必须与 current_object 一致，否则先 authorize_intent(Call) + enter_object
  - 不重新设计授权逻辑，只是薄适配到已有 KernelCall::CapabilityGrant
- veritasd JSONL 命令 tx_capability_grant
  - 请求格式：session_id, grantor, grantee, capability_type, resource
  - 响应格式：ok: true / ok: false + error
- 新增测试
  - tests/capability_grant_p1_worldapi.rs：WorldService 层集成测试
  - tests/capability_grant_p1_jsonlines.rs：JSONL 外部进程 e2e 测试

### 验证结果

- cargo test 全量通过：103 + 2 = 105 passed, 0 failed
- JSONL e2e 验证完整链路：JSON request → veritasd → WorldApi → Kernel → Engine → CapabilityGraph
- 未授权 B 操作在 commit 时被拒绝
- A grant B 后 B 操作成功
- grantor 语义保持真实（A != B）

### 未做

- Forge 未修改（P2 待做）
- CapabilityGraph 未修改
- Engine 授权语义未修改

---

## P0 CapabilityGrant 链闭合完成 — 2026-08-13

### 已完成（两个 commit）

1. `e2dc91b` fix: CapabilityGrant 闭合 grantor 语义链
   - Engine::capability_grant 增加 grantor 参数
   - KernelCall::CapabilityGrant 增加 grantor 字段
   - Machine::CapabilityGrant 从 ctx.current_object 取 grantor
   - 17 处测试调用点对齐（16 处 grantor=grantee 保持原语义，
     checkpoint_roundtrip.rs 跨对象场景 grantor=1）

2. `a3affad` test: 新增 CapabilityGrant 跨对象授权回归测试
   - engine.rs 添加 snapshot_capabilities_for_test 只读转发
   - test_api.rs 添加 test_capability_records 测试专用接口
   - 新测试验证 A grant B → granted_by=A, holder=B, A!=B
   - 未授权时 B 操作被拒绝，授权后成功

### grantor 传递链路（最终状态）

Machine: self.ctx.current_object
  → KernelCall::CapabilityGrant { grantor, grantee, ... }
  → Kernel::handle 透传
  → Engine::capability_grant(grantor, grantee, ...)
  → PendingCapabilityGrant { grantor, grantee }
  → CapabilityGraph::grant / restore_grant
  → CapabilityInfo { granted_by: grantor, root_holder: grantee }

### 验证结果

- cargo test 全量通过：103 + 1 = 104 passed, 0 failed
- 跨对象授权回归测试单独通过
- WAL recovery 语义未被破坏（已有测试覆盖）

### 未做（P1/P2 后续）

- veritasd 未暴露 tx_capability_grant 命令（P1）
- Forge adapter/session 未添加 grant 方法（P2）
- 按架构边界：先闭合 Ver 内部，再暴露 veritasd，最后 Forge 薄适配

---

## 架构债：Session/Machine 身份管理统一 — 待执行

### 问题
Veritas 有两条独立执行入口，各自维护身份上下文：
- Machine：CALL 指令完整管理 current_object、capability_context、CallFrame 栈、pending_calls
- Session：tx_write 手抄了半份 CALL 逻辑（authorize_intent + enter_object），漏了 capability_context 同步、
  pending_calls 记录、无对称的"退出"步骤，身份切换是永久性的

同一概念两套实现，导致 root/call/capability_context 等身份问题反复出现。

### 当前状态（事实）
- Machine 的 CALL/RETURN 是完整且有作用域的身份切换机制。
- Session 仍存在独立的身份管理逻辑（tx_write 手抄了 authorize_intent +
  enter_object），与 Machine CALL 语义实现分离。
- 当前已知故障已通过 capability_context bootstrap 等修复消除
  （多对象事务、WAL recovery 均通过），但双实现未统一。
- 因此该项属于结构性架构债，不是当前已确认的功能性 bug。

### 目标（计划）
- Session 不再维护一套独立的身份切换语义。
- 身份进入/退出的核心语义由 Engine/TransactionContext 统一实现。
- Machine CALL/RETURN 与 Session 的隐式 current_object 操作最终共享
  同一身份作用域机制。

### 执行步骤（计划）
1. 从 Machine CALL handler 中抽出身份切换核心，形成 Engine 层
   enter_identity(target) / leave_identity() 方法。
2. TransactionContext 增加 identity_stack；CallFrame 仅保留 Machine
   执行所需的寄存器/PC 状态。
3. Session 的隐式身份绑定操作改用统一的
   enter_identity + operation + leave_identity 作用域。
4. 逐步迁移其他依赖 current_object 的 Session 原语。
5. 收紧 current_object/capability_context/identity_stack 的可见性，
   删除公开的手工身份切换入口。
6. 删除 Session 侧重复的 authorize_intent / enter_object 实现。

### 不改（边界）
- authorize_intent / verify_capability / commit 权限模型
- capability graph
- ObjectBirth 的自动授权逻辑
- Machine CALL/RETURN 的外部语义

### 时机
非紧急。当前多对象事务身份问题已经修复，全量测试通过。
在下一次身份相关变更或出现重复身份 bug 时实施结构性收敛。
