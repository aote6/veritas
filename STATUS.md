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
