=== Veritas Kernel STATUS ===

Date: 2026-08-03
Branch: main

## Current milestone

P28.1 Instruction Dispatch Closure -- Complete
P28.1.1 Audit Tool -- Complete

Machine::step() now correctly dispatches all 30 Instruction variants
through three execution classes. Previously, 5 kernel instructions
(Abort, CapabilityGrant, Effect, Savepoint, RollbackTo) were defined
in the ISA but unreachable from Machine — they fell through to a
wildcard no-op.

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
P28.0         - Cross-module TRAP persistence test:
                Module A (TRAP OBJECT_BIRTH → COMMIT → HALT)
                Module B (TRAP OBJECT_FREEZE → COMMIT → HALT)
                verifies Kernel as World across Runtime::execute boundaries
P28.1         - Machine dispatch closure: Abort, CapabilityGrant, Effect,
                Savepoint, RollbackTo now reach Kernel API from Machine::step()
P28.1.1       - Audit tool v2: proper function-body parsing, three-class
                execution model output

### Instruction Execution Architecture (P28.1)

All 30 Instruction variants now reachable through three classes:

  Class A — CPU local / Kernel API: 23 dispatched in Machine::step()
    Add, Sub, Cmp, LoadConst, LoadStateU64, LoadStateBytes,
    WriteRegister, Jmp, Jz, Jnz, Jn, Call, Return, Trap, HostCall,
    Halt, Nop, Commit, Abort, CapabilityGrant, Effect, Savepoint,
    RollbackTo

  Class B — Kernel legacy: 2 dispatched in execute_kernel_instruction()
    Read, Write

  Class C — Trap ABI: 5 routed via Trap → KernelCall::decode() → Kernel::handle()
    ObjectBirth, ObjectDeath, ObjectFreeze, ObjectLink, ObjectUnlink

Audit tool: tools/audit_instruction_dispatch.py — CI-runnable,
exits 0 when 30/30 reachable, 1 on gap.

## Explicit non-goals

P8.4 Death Event Dispatcher = structural refactor only (deferred)
No new death semantics without constitution change
Object lifecycle instructions stay Trap ABI — no direct dispatch to
  preserve CPU/Kernel layering

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
  not enforced yet; blocked on tests/object/lifecycle.rs having two
  direct calls with no Instruction/Executor path to migrate to
WAL mid-write crash (truncated file between Effect entries and the
  Commit entry) untested -- existing recovery tests only cover clean
  full-restart scenarios, not partial-write crashes
Commit instruction duplicated in both step() and execute_kernel_instruction()
  — harmless but should be cleaned when eki() legacy path is audited
execute_kernel_instruction() still exists as legacy path for Read/Write;
  migration to step() deferred until full audit complete

## Next milestone candidates

1. P29 WAL Replay / Crash Recovery — prove world state can be
   reconstructed from history; mid-write crash tests
2. Trap ABI hardening — validate Object lifecycle through Trap ABI:
   parameter encoding stability, error propagation, WAL recording,
   abort cleanup, replay recoverability
3. Constitution gap analysis (deferred from previous milestone)

## Documentation map

See README. Do not duplicate instruction counts or test totals here;
CI is source of truth for green builds.
