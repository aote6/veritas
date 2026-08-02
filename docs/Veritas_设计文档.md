# Veritas Design Specification

## 1. Problem

Hardware does not negotiate. Most software stacks do: names, layers, and trust
assumptions sit between what was requested and what is true.

Veritas asks: what would it mean for software state evolution to inherit
the non-negotiable character of hardware, not speed but inability to lie about
what happened.

## 2. Abstract machine layers

Instruction -> Verified execution -> Transaction -> Deterministic transition -> State root

- Transaction is the atomic unit of change and the execution context.
- Object is the unit of identity, memory ownership, and link endpoints.
- ModuleObject is a frozen template of code; instances are Objects.
- Capability is a kernel resource (not an Object) authorizing access to a resource Object.
- Link is a typed relation between Objects with death semantics.

## 3. Why Runtime Object

State, authority, and topology must share one identity space. A Module-centric
world (Module holds state, Module is capability holder) collapses template and
instance and breaks isolation.

Therefore:
- Only Objects own MemorySpace and appear in links.
- ModuleObject supplies code; it does not own mutable instance state.
- Execution current_object is always an ObjectId inside a Transaction.

## 4. Why three LinkTypes

| Type | Role on death |
|------|----------------|
| OWNS | Lifecycle coupling: owner dies, owned must die |
| DEPENDS_ON | Dependency signal: dependency dies, dependent is notified (DependencyInvalidated); dependent stays alive |
| REFERENCES | Weak edge: remove only |

Conflating these loses the ability to express ownership vs dependency vs weak ref.

## 5. Why Capability is not an Object

Capabilities are grants over resources. Treating them as Objects creates
recursive authority problems. Kernel owns the capability graph; Objects are
holders and resources.

Invalidation when a resource dies is a liveness property checked at
use (lazy), not necessarily an eager graph sweep at death time.

## 6. Determinism and recovery

Same program plus same inputs gives same state root. WAL records commits and
object/link/effect facts so recovery rebuilds registry, topology, and retries
unacked effects.

## 7. Non-goals

- Competing with general-purpose VMs on throughput
- Implicit trust in operators or agents
- Silent default changes to commit/begin/write behavior

## 8. Normative references

Field layouts: 运行时数据模型标准
Object guarantees: Runtime Object 规范
Hard constraints: constitution/
