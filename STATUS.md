=== Veritas Kernel STATUS ===

Date: 2026-08-02
Branch: main

## Current milestone

P8 Object Death -- Semantic Complete

Object death is a lifecycle event, not a single-field state flip.

## Completed

Transaction   - BEGIN/COMMIT/ABORT, OCC, snapshot isolation, savepoint
State         - Address=(ObjectId,StateId), read/write sets, state root
Object        - Birth / Freeze / Death; ObjectRecord first-class
Link OWNS     - Owner death to owned death (transitive closure)
Link DEPENDS_ON - Dependency death to DependencyInvalidated
Link REFERENCES - Edge removal only
Capability    - Graph grant/delegate/revoke; lazy resource liveness at use
WAL           - Commit/birth/death/link/effect; recovery rebuild
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

## Next milestone

Re-scan six constitutions for semantic gaps (not commit()-driven features).
Candidate themes: Module lifecycle, recovery completeness, link ops beyond death.
Numbering (P9+) only after gap analysis.

## Documentation map

See README. Do not duplicate instruction counts or test totals here;
CI is source of truth for green builds.
