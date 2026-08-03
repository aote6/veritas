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
- 176 tests pass

### Remaining Step 2 tech debt

- state_map/apply_records is now redundant for recovery state_store init;
  apply() step 1 writes the same data again. Clean up by letting apply()
  fully own state_store init, keeping apply_records only for scope_map/effects.
- Missing test: cross-tx unlink-then-death recovery (tx1 link, tx2 unlink, tx3 death)
  to verify topology correctly reflects unlink before closure recomputation.
- Missing test: cross-tx unlink-then-death recovery (tx1 link, tx2 unlink, tx3 death)
  to verify topology correctly reflects unlink before closure recomputation.

## Step 3 (atomic WAL write) completed

- WalEntry::TransactionCommitted(TransactionDelta) — single atomic WAL entry
- commit(): 7 independent append_and_sync → 1 TransactionCommitted + apply()
- Recovery: with_wal_path() handles TransactionCommitted alongside legacy WalEntry::Commit
- CRC truncation test for new format (test_truncated_transaction_committed_discarded)
- Orphan test updated: corrupted TransactionCommitted discarded by CRC check
- Critical #1 (Commit non-atomicity) resolved ✅
- 176 tests pass


## Known gaps

engine.rs size / modular split deferred
DEPENDS_ON carrier may evolve (Effect/Trap); event semantics fixed
Module lifecycle (instance vs template) not fully closed vs constitution
engine.topology vs src/graph dual stores
capability_enforced default false (transition)
Eager capability purge on death: not required (lazy is normative)
grant_base_access (begin_in_object) still grants capability_graph directly
object_birth still `pub`, not `pub(crate)`
Commit instruction duplicated in step() and execute_kernel_instruction()
execute_kernel_instruction() legacy path for Read/Write
ReplayRecord missing Object/Link/Capability — ReplayEngine is StateMemory-only

## Next milestones

1. P30.4 — ReplayRecord upgrade: Object/Link/Capability in replay entries (now TransactionDelta-based)
2. P30.5 — ReplayEngine full world replay + Receipt verification
3. P31   — Checkpoint / Snapshot
4. Effect retry logic on recovery (Step 3.5) — Commit exists but EffectAck missing → re-enqueue
5. Cross-tx unlink-then-death recovery boundary test
6. Clean up state_map/apply_records dual path for state_store init

## Documentation map

See README. 176 tests pass.
