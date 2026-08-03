=== Veritas Kernel STATUS ===

Date: 2026-08-03
Branch: main

## Current milestone

P29 WAL Recovery Verification — Three-layer test suite complete

Recovery testing elevated from "a few regression tests" to a
layered verification of the recovery algorithm itself:

  P29.1 — Basic Recovery: birth, link, abort survive crash
  P29.2 — Recovery Invariants: lifecycle state-machine correctness
  P29.3 — Recovery Equivalence: EngineSnapshot(A) == EngineSnapshot(B)

## Completed

Transaction   - BEGIN/COMMIT/ABORT, OCC, snapshot isolation, savepoint
State         - Address=(ObjectId,StateId), read/write sets, state root
Object        - Birth / Freeze / Death; ObjectRecord first-class
Link OWNS     - Owner death to owned death (transitive closure)
Link DEPENDS_ON - Dependency death to DependencyInvalidated
Link REFERENCES - Edge removal only
Capability    - Graph grant/delegate/revoke; lazy resource liveness at use
WAL           - Commit/birth/death/link/freeze/unlink/capability-grant/effect;
                recovery rebuild reconstructs object_registry, topology,
                capability_graph (grant + revoke), not just state/scope
Machine       - Step/run, Call/Return same tx, trap frame
Assembler     - Present; Module-as-template path still evolving
P8            - Object Death semantic complete
P4.x/P5.x     - Capability/Freeze/Unlink WAL-durable
Phase 1 Step 7 - KernelCall ABI codec: decode() + handle() + TrapResult
Phase A       - Kernel lifecycle detached from Runtime::execute();
                Kernel persists across multiple execute() calls
P28.0         - Cross-module TRAP persistence test
P28.1         - Machine dispatch closure: 5 kernel instructions reach API
P28.1.1       - Audit tool v2: three-class execution model, 30/30 reachable
P29.1         - WAL Recovery basic tests: birth/link/abort survive crash
P29.2         - WAL Recovery invariants: 6 state-machine correctness proofs
P29.3         - WAL Recovery equivalence: snapshot comparison Engine(A)==Engine(B)
                Added Engine::list_object_ids() + Kernel::list_object_ids()

### Instruction Execution Architecture (P28.1)

All 30 Instruction variants reachable through three classes:

  Class A — CPU local / Kernel API: 23 dispatched in Machine::step()
  Class B — Kernel legacy: 2 dispatched in execute_kernel_instruction()
  Class C — Trap ABI: 5 routed via Trap → KernelCall::decode() → Kernel::handle()

Audit tool: tools/audit_instruction_dispatch.py — 30/30 reachable.

### Recovery Test Suite (P29)

  9 recovery tests across 3 test files:
    tests/wal_recovery_object.rs       (3 tests, P29.1)
    tests/wal_recovery_invariants.rs   (6 tests, P29.2)
    tests/wal_recovery_equivalence.rs  (5 tests, P29.3) — full snapshot compare

  Recovery coverage matrix:
    Object ✅  Link ✅  Capability ✅  Freeze ✅  Unlink ✅  Abort ✅

  P29.3 EngineSnapshot compares: object_ids, object_states, links, state_root.
  Future: expand to capability_graph, scope_registry, global_version.

## Explicit non-goals

P8.4 Death Event Dispatcher = structural refactor only (deferred)
No new death semantics without constitution change
Object lifecycle instructions stay Trap ABI — no direct dispatch

## Known gaps

engine.rs size / modular split deferred
DEPENDS_ON carrier may evolve (Effect/Trap); event semantics fixed
Module lifecycle (instance vs template) not fully closed vs constitution
engine.topology vs src/graph dual stores
capability_enforced default false (transition)
Eager capability purge on death: not required (lazy is normative)
grant_base_access (begin_in_object) still grants capability_graph
  directly, not via pending_capabilities -- intentional, same-tx
  visibility needed for verify_capability; not a leak, low priority
object_birth still `pub`, not `pub(crate)` -- Machine-sole-entry-point
  not enforced yet
WAL mid-write crash (truncated file between Effect entries and Commit)
  untested — existing recovery tests cover clean full-restart only
Commit instruction duplicated in step() and execute_kernel_instruction()
  — harmless but should be cleaned
execute_kernel_instruction() legacy path for Read/Write
RecoveryManager::apply_records ignores Object/Link/Capability WAL entries
  — VeritasEngine::with_wal_path() does its own second-pass rebuild;
  dual recovery paths are a known architectural debt
EngineSnapshot does not yet include capability_graph, scope_registry,
  or global_version — expand for complete equivalence coverage

## Next milestone candidates

1. P29.4 — Random/Property WAL Replay testing
   Generate random operation sequences → execute → crash →
   recover → compare snapshots; run hundreds of rounds
2. P29.5 — Mid-write crash recovery (truncated WAL)
3. Trap ABI hardening — Object lifecycle through Trap ABI validation
4. Constitution gap analysis

## Documentation map

See README. 149 tests pass.
