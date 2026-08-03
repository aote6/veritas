=== Veritas Kernel STATUS ===

Date: 2026-08-03
Branch: main

## Current milestone

P30 Deterministic Replay — World state replay verified; Capability identity gap identified

P30.1: Same WAL → identical Engine state (4 determinism tests)
P30.2: WAL contains full world state — object/link/capability replay (2 tests)
P30.3 (NEXT): Capability Identity Replay — grant_sequence must survive recovery

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
  P30.3 — Capability Identity Replay ❌ (NEXT)

  Known gap: CapabilityGraph::new() resets grant_sequence to 1.
  Recovered cap_graph replays grants correctly (the edges exist),
  but CapabilityIds change. This means:
    - holds_capability(cap_id_before, holder) returns false after recovery
    - capability_sequence() is lower after recovery than before crash
  Root cause: grant_sequence is not persisted to WAL or restored.
  Fix: persist grant_sequence in WAL Checkpoint or restore from
    max cap_id in replayed CapabilityGrant entries.

### ReplayRecord Gap

  ReplayRecord currently only records: writes, before_root, after_root.
  Does NOT record: object births, links, capability grants, lifecycle events.
  Source comment: "ReplayRecord目前不记录object_id，统一归属内核Object(0)"
  This means the old ReplayEngine (src/replay.rs) only replays StateMemory,
  not the Veritas world. Real ReplayEngine must be upgraded to handle
  the full WalEntry vocabulary.

## Explicit non-goals

P8.4 Death Event Dispatcher = structural refactor only (deferred)
No new death semantics without constitution change
Object lifecycle instructions stay Trap ABI

## Known gaps

engine.rs size / modular split deferred
DEPENDS_ON carrier may evolve (Effect/Trap); event semantics fixed
Module lifecycle (instance vs template) not fully closed vs constitution
engine.topology vs src/graph dual stores
capability_enforced default false (transition)
Eager capability purge on death: not required (lazy is normative)
grant_base_access (begin_in_object) still grants capability_graph directly
object_birth still `pub`, not `pub(crate)`
WAL mid-write crash untested
Commit instruction duplicated in step() and execute_kernel_instruction()
execute_kernel_instruction() legacy path for Read/Write
RecoveryManager::apply_records ignores Object/Link/Capability WAL entries
  — VeritasEngine::with_wal_path() does second-pass rebuild (dual path)
grant_sequence not persisted across recovery — CapabilityIds change (P30.3)
ReplayRecord missing Object/Link/Capability — ReplayEngine is StateMemory-only

## Next milestones

1. P30.3 — Capability Identity Replay: grant_sequence WAL persistence
2. P30.4 — ReplayRecord upgrade: Object/Link/Capability in replay entries
3. P30.5 — ReplayEngine full world replay + Receipt verification
4. P31   — Checkpoint / Snapshot

## Documentation map

See README. 165 tests pass.
