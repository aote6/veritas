# Veritas Runtime Data Model Standard

Single source of truth for structural types. Semantics of guarantees live in
Runtime Object 规范 and constitution/.

## Identifiers

ObjectId    - Runtime identity (links, holder, resource, current_object, scope owner)
ModuleId    - Optional alias for ModuleObject template identity; not used as link endpoint or capability holder
StateId     - Slot within an Object memory space
Address     - (ObjectId, StateId), only legal memory location key
TxId, ScopeId, CapabilityId - As implemented

## ObjectRecord

Logical fields (names may match Rust; this doc is authoritative for shape):

- id: ObjectId
- object_type: StateObject | ModuleObject
- state: Alive | Frozen | Dead
- body: State | Module { code_section, import_section, export_section, verification_rule? }

Constructors (conceptual): new_state(id), new_module(...)

## LinkEdge

- from: ObjectId
- to: ObjectId
- link_type: DependsOn | Owns | References

No self-loops. Direction is meaningful (OWNS: owner to owned).

## TransactionContext (structural)

- tx_id, snapshot_version
- read_set / write_set keyed by Address
- scope_write_set, effect_queue, savepoints
- pending_objects, pending_links, pending_unlinks, pending_freezes, pending_deaths
- capabilities: list of CapabilityId
- capability_enforced: bool
- current_object: ObjectId
- aborted: bool

## Capability (kernel resource, not Object)

- CapabilityId, capability_type, granted_by, root_holder, resource: ObjectId
- Delegation forest per capability_id (holder active flags, edges)

## Topology

Committed edges: Vec/Store of LinkEdge (engine topology; graph/ may exist as
parallel experimental store, do not dual-write semantics without convergence plan).

## WAL record kinds (logical)

Commit, ObjectBirth, ObjectDeath, ObjectLink, Effect, EffectAck, scope changes,
checkpoint-related as implemented.

## Encoding notes

DependencyInvalidated payload convention (when emitted as effect):
idempotency_key: dep-inv:{tx_id}:{dependent}:{dependency}
payload: le-bytes dependent || dependency
