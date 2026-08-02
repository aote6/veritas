=== Veritas Kernel STATUS ===

Date: 2026-08-02
Branch: main

## Current milestone

P8 Object Death -- Semantic Complete
P4.x/P5.x Recovery Correctness -- Capability/Freeze/Unlink now WAL-durable

Object death is a lifecycle event, not a single-field state flip.
Recovery must reconstruct full runtime state, not just Commit/writes:
Capability grants, Freeze, and Unlink previously vanished on restart.

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

## Explicit non-goals

P8.4 Death Event Dispatcher = structural refactor only (deferred)
No new death semantics without constitution change

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

## Next milestone

Re-scan six constitutions for semantic gaps (not commit()-driven features).
Candidate themes: Module lifecycle, recovery completeness, link ops beyond death.
Numbering (P9+) only after gap analysis.

## Documentation map

See README. Do not duplicate instruction counts or test totals here;
CI is source of truth for green builds.
