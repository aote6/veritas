=== Veritas Kernel STATUS ===

Date: 2026-08-03
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
- 184 tests pass

### Remaining Step 2 tech debt

- state_map/apply_records is now redundant for recovery state_store init;
  apply() step 1 writes the same data again. Clean up by letting apply()
  fully own state_store init, keeping apply_records only for scope_map/effects.
- Missing test: cross-tx unlink-then-death recovery (tx1 link, tx2 unlink, tx3 death)
  to verify topology correctly reflects unlink before closure recomputation.

## Step 3 (atomic WAL write) completed

- WalEntry::TransactionCommitted(TransactionDelta) — single atomic WAL entry
- commit(): 7 independent append_and_sync → 1 TransactionCommitted + apply()
- Recovery: with_wal_path() handles TransactionCommitted alongside legacy WalEntry::Commit
- CRC truncation test for new format (test_truncated_transaction_committed_discarded)
- Orphan test updated: corrupted TransactionCommitted discarded by CRC check
- Critical #1 (Commit non-atomicity) resolved ✅
- 184 tests pass


## Known gaps

### From audit — not yet addressed this cycle

**Capability** (Critical #4):
- capability_enforced defaults to true, but the toggle still exists and can disable all checks
- grant_base_access (begin_in_object) injects directly into cap_graph, bypassing normal Grant path
- verify_capability only covers write_set, not reads/links/death/freeze/cross-object calls

**Kernel boundary** (Critical #3):
- object_birth/object_death/object_link/capability_grant/commit/abort are pub fn, callable directly without TRAP
- Kernel is still a thin pass-through wrapper; no User/Kernel mode enforcement
- Machine direct Instruction::Commit/Abort path and TRAP → KernelCall path both exist (two dispatch paths)

**ObjectId allocation**:
- ObjectId still provided by caller, not assigned by Kernel
- Dependency on committed-tx determination is now satisfied (Step 3 complete); ready to schedule
- Boundary: only count Births from TransactionCommitted entries, not orphaned ones

**Dual stores**:
- src/graph/ exists as an independent module (GraphStore/Journal/Policy/ReplayEngine) with zero callers outside its own tree
- engine.topology (Mutex<Vec<LinkEdge>>) is the active implementation; src/graph is dead code, not an active dual store

**Instruction dispatch**:
- Two active paths: Machine direct match on Instruction::Commit/etc, TRAP → KernelCall
- Legacy execute_kernel_instruction() is now a no-op stub (all logic marked "Handled inline" or "now handled inline in step()"), but the function still exists

**Module lifecycle**:
- ModuleObject/ModuleInstance separation not fully closed per constitution
- ModuleInstance as StateObject subtype, auto-creation on load, death notification still partial

**Savepoint**:
- Structure exists, full semantics not implemented; constitution declares "future extension"

### From this refactoring cycle — completed

### From this refactoring cycle — remaining

**state_map/apply_records dual path**:
- Recovery: state_map computed by apply_records → StateStore::from_map()
- Then apply(&delta) writes delta.writes again into state_store
- Functionally harmless (same data), but violates "one truth source" principle this refactor established

**Missing boundary tests**:
- Cross-tx unlink-then-death: tx1 link → tx2 unlink → tx3 death → recovery should NOT cascade
- Exercise topology correctness across multiple apply() calls in recovery
- No known bug, but no test coverage for this ordering

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
3. ✅ Cross-tx unlink-then-death recovery boundary test (2 tests, 178 total)
4. ✅ Cleanup: unused imports, dead code in test_truncated_transaction_committed_discarded

### Short-term
4. P30.4 — ReplayRecord upgrade: Object/Link/Capability (now TransactionDelta-based)
5. P30.5 — ReplayEngine full world replay + Receipt verification
6. Clean up state_map/apply_records dual path
7. ⚠️ ObjectId allocation: allocator done, TRAP path correct; pub fn bypass path remains (P1b dependency)

### Medium-term (from audit Critical findings)
7. Capability always-on: remove capability_enforced toggle, unify all access checks
8. Kernel boundary: close non-TRAP entry points (pub → pub(crate)), single TRAP dispatch
9. Remove src/graph/ dead code; engine.topology is the sole topology store

### Later
10. P31 — Checkpoint / Snapshot
11. ModuleObject/ModuleInstance lifecycle closure
12. Unify instruction dispatch paths (Machine direct + TRAP)
13. Savepoint full semantics

## Documentation map

See README. 184 tests pass.
