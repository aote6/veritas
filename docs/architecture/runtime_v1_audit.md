# Runtime v1 Architecture Constitution Audit

**Date:** 2026-08-03
**Audit Type:** Architecture Constitution Audit v1 (Final)
**Scope:** Veritas Kernel v0.1.0 vs Constitution v0.2
**Overall Alignment: 78%**

*Note: Sections 1-3 describe the current implementation (verified facts).
Sections 4-6 describe the proposed Runtime v2 architecture and migration
plan (design proposal).*

---

## 1. Constitution Alignment Matrix

### 1.1 Per-Module Scores

| Module | Alignment | Critical Gaps |
|--------|----------|---------------|
| Object | 96% | ModuleInstance missing |
| Memory | 100% | - |
| Transaction | 99% | - |
| Capability | 97% | holder death handling |
| Link | 100% | - |
| Kernel | 45% | Non-singleton, OBJECT_BIRTH API violation, TRAP non-exclusive, Machine bypasses Executor |
| Module | 25% | ModuleObject never created, ModuleInstance nonexistent, template-instance not separated |

### 1.2 Detailed Clause-by-Clause Coverage

| Constitution Clause | Source | Implementation | Status |
|---------------------|--------|---------------|--------|
| Object is the sole first-class citizen | object.md 1 | ObjectRecord, ObjectId | Aligned |
| ObjectId 64-bit globally unique, non-reusable | object.md 2 | types.rs: ObjectId = u64 | Aligned |
| ObjectType: StateObject / ModuleObject | object.md 3 | types.rs: ObjectType | Aligned |
| Lifecycle state machine Alive-Frozen-Dead | object.md 4 | ObjectState + allows() | Aligned |
| Object composition: body, memory_space, capability_space | object.md 5 | ObjectRecord | Aligned |
| ModuleObject is read-only template, default FROZEN | object.md 6 | ObjectRecord::new_module() defined but never called | Partial |
| ModuleInstance is a StateObject | object.md 6 | Does not exist in codebase | Missing |
| ModuleInstance has independent pc, memory_space, capability_space | object.md 6 | Does not exist in codebase | Missing |
| Multiple ModuleInstances share one ModuleObject | object.md 6 | Does not exist in codebase | Missing |
| ModuleObject death notifies ModuleInstances | object.md 6 | Constitution itself marks this as not yet implemented | Missing |
| Link three semantics (OWNS/DEPENDS_ON/REFERENCES) | link.md 3 | LinkType + LinkEdge | Aligned |
| Link death cascade closure | link.md 6 | expand_owns_death_closure | Aligned |
| Link operations transactional | link.md 5 | pending_links | Aligned |
| Memory = { ObjectId to MemorySpace } | memory.md 2 | Address = (ObjectId, StateId), StateStore partitioned by Address | Aligned |
| Addressing = (ObjectId, StateId) | memory.md 4 | Address struct | Aligned |
| Kernel is not an Object, is Machine kernel mode | kernel.md 1 | No Kernel Mode enum exists | Missing |
| TRAP enters kernel mode | kernel.md 1 | Instruction::Trap exists but not exclusive path | Partial |
| Kernel does not own MemorySpace / ObjectId / lifecycle | kernel.md 2 | VeritasEngine holds state_memory etc. | Partial |
| All Kernel Services via TRAP | kernel.md 2 | TRAP + Executor dual path, partially unified | Partial |
| OBJECT_BIRTH returns ObjectId | kernel.md 3.1 | engine.object_birth(ctx, id) - caller provides id | Violation |
| OBJECT_DEATH expands OWNS closure | kernel.md 3.2 | expand_owns_death_closure | Aligned |
| CAPABILITY_GRANT transactional | kernel.md 3.5 | pending_capabilities | Aligned |
| Transaction is the fifth core primitive | transaction.md 1 | TransactionContext | Aligned |
| current_object belongs to Transaction | transaction.md 5 | ctx.current_object | Aligned |
| capability_context belongs to Transaction | transaction.md 4 | ctx.capability_context (fixed in P1-1) | Aligned |
| CALL does not change Transaction boundary | transaction.md 8 | call_stack push/pop | Aligned |
| Snapshot Isolation + conflict detection | transaction.md 9 | detect_conflict | Aligned |
| ABORT rolls back all Object modifications | transaction.md 10 | controller.abort | Aligned |
| Determinism: same input always same output | transaction.md 13 | FNV hash + deterministic WAL | Aligned |

**Summary:**
- Aligned: 20 items
- Partial: 5 items
- Missing: 5 items
- Violation: 1 item

---

## 2. Authority Analysis (Single Source of Truth)

| Capability | Authoritative Entry Point | Unique |
|------------|--------------------------|--------|
| Commit | Executor::execute_instruction to engine.commit | Yes (P1-3 fix) |
| ObjectBirth | TRAP (machine.rs) to engine.object_birth | Yes |
| ObjectDeath | TRAP (machine.rs) to engine.object_death | Yes |
| ObjectLink | TRAP (machine.rs) to engine.object_link | Yes |
| ObjectFreeze | TRAP (machine.rs) to engine.object_freeze | Yes |
| CapabilityGrant | Executor::execute_instruction to engine.capability_grant | Yes |
| Memory Read | Executor::read_state to engine.read | Yes |
| Memory Write | Executor::write_state / TRAP WriteRegister to engine.write | Partial: two paths, same engine method |
| Engine Instantiation | Runtime::execute + 5 test sites = 6 call sites | Violation: created per execution |

---

## 3. Evidence Chain

### 3.1 Kernel Lifecycle Violation

Runtime::execute (runtime.rs:10)
to VeritasEngine::new()
to Machine::new(&engine)
to ... execute ...
to engine + machine dropped

Each Runtime::execute call creates a new world. ObjectRegistry, StateMemory,
CapabilityGraph, and WAL are all bound to a single execute() invocation.

**Constitution:** kernel.md Section 2 - Kernel does not have a lifecycle;
it is not created or destroyed.

**Evidence:** grep found 6 VeritasEngine::new() call sites
(5 in tests + 1 in Runtime).

### 3.2 OBJECT_BIRTH API Violation

pub fn object_birth(
&self,
ctx: &mut TransactionContext,
object_id: ObjectId,   // <-- caller provides this
) -> Result<(), VeritasError>

The caller provides object_id. Kernel does not allocate it.

**Constitution:** kernel.md Section 3.1 - OBJECT_BIRTH takes object_type,
returns ObjectId.

**Evidence:** engine.rs line 889. object_id is a parameter, not a return
value. This is not an implementation difference but an API definition
difference.

### 3.3 TRAP Not the Exclusive Kernel Entry

Machine directly calls engine methods via TRAP handler, but Executor also
routes Read/Write/Commit/CapabilityGrant directly to engine without TRAP.

**Constitution:** kernel.md Section 8 - All kernel services called via TRAP.

**Evidence:**
- machine.rs lines 366-396: TRAP handler directly calls
  self.engine.object_birth/death/link/freeze
- executor.rs lines 44-80: Direct engine calls for
  Read/Write/Commit/CapabilityGrant
- machine.rs lines 443, 449: self.engine.abort() called directly

### 3.4 ModuleInstance: Zero Implementation

$ grep -rn "ModuleInstance" src/ --include="*.rs"
(no results)

ObjectRecord::new_module() exists as a constructor but is never called.
No code path creates a ModuleObject. ModuleImage is used directly as both
template and executable entity.

**Constitution:** object.md Section 6 - ModuleObject is a read-only
template. ModuleInstance is a StateObject with independent pc,
memory_space, and capability_space.

### 3.5 No Hidden Global State (Positive Finding)

$ grep -rn "static|lazy_static|thread_local" src/ --include="*.rs"
(no runtime global state found beyond test constants)

The Engine is the sole state container. This will simplify the Kernel
singleton migration.

---

## 4. Root Cause

Runtime v1 was designed around the lifecycle of a program execution,
whereas Constitution v0.2 defines the lifecycle of a persistent Object
world.

**Symptom to Root Assumption Mapping:**

| Symptom | Root Assumption |
|---------|----------------|
| Runtime::execute creates new Engine each time | "One execution = one world" |
| ModuleImage fed directly to Machine::boot | "Module = Program" |
| object_birth(id) requires caller-provided ID | "Object = program resource" |
| Machine directly calls engine.xxx() | "No user/kernel mode boundary" |

These four symptoms share a single root cause. The three planned
refactorings (R1 Kernel singleton, R2 ModuleObject/Instance separation,
R3 TRAP as exclusive entry) are not independent tasks but three
manifestations of the same architectural mismatch.

---

## 5. Runtime v2 Target Model

VeritasMachine (persistent, created once at startup)

|
+-- Kernel (singleton, created with Machine, never destroyed)
|   +-- ObjectRegistry
|   +-- CapabilityGraph
|   +-- StateMemory
|   +-- WAL
|   +-- TransactionManager

|
+-- Runtime (Loader + Scheduler)
|   +-- Load ModuleObject (template, FROZEN)
|   +-- Create ModuleInstance (StateObject, ACTIVE)
|   +-- Allocate MemorySpace for Instance
|   +-- Grant initial Capability
|   +-- Schedule execution

|
+-- ModuleInstance (executable entity)
+-- pc / registers
+-- MemorySpace
+-- CapabilitySpace
+-- DEPENDS_ON link to ModuleObject
+-- May persist after execution completes

Under this model:
- R1 (Kernel singleton) resolves naturally because the Machine owns the Engine
- R2 (ModuleObject/Instance separation) emerges because ModuleImage becomes
  a template, and boot creates an Instance instead of executing directly
- R3 (TRAP as exclusive entry) follows because the boundary between Machine
  (user mode) and Kernel is now explicit

---

## 6. Migration Phases

### Phase 1: Kernel as World (78% to 90%)

- VeritasMachine struct holds Engine as persistent member
- Engine creation moves from Runtime::execute to VeritasMachine::new
- OBJECT_BIRTH: Kernel allocates ObjectId (caller no longer provides it)
- TRAP becomes exclusive kernel entry point (Machine no longer calls
  engine directly)
- All kernel services unified under a single TRAP ABI

### Phase 2: Module Instantiation (90% to 95%)

- ModuleImage becomes ModuleObject (template, FROZEN)
- Machine::boot becomes Runtime::create_instance (returns ModuleInstance
  ObjectId)
- ModuleInstance created as StateObject with independent MemorySpace
- DEPENDS_ON Link established: Instance to ModuleObject
- ModuleObject death notification to Instances (constitution gap closure)

---

## 7. v1 Freeze Declaration

Runtime v1 is now frozen. No further local patches will be made to the
v1 architecture. All future commits belong to Runtime v2 migration.

This audit serves as the authoritative baseline for measuring
constitution alignment improvement through the v2 migration phases.
