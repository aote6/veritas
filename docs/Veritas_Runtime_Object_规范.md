# Veritas Runtime Object Guarantees

Normative machine guarantees. Structures: 数据模型标准. Why: 设计文档.

## Identity

- ObjectId is globally unique and never reused after death.
- Death does not free the id for a new live object.

## Birth

- OBJECT_BIRTH only inside a Transaction.
- Abort leaves no registry trace.
- Commit installs ObjectRecord (typically Alive for state objects).
- Creator receives AdminCap (or equivalent) for that Object as resource.

## Lifecycle states

- Alive: readable/writable per rules; may link.
- Frozen: no mutation; death still allowed.
- Dead: terminal; no write/freeze; cannot be target of new links.

## Isolation

- Memory access is always via Address (ObjectId, StateId).
- current_object supplies ObjectId for implicit operands.
- No bare StateId global heap.

## Death event (semantic)

On commit of death set D (after OWNS closure):

1. Every id in D becomes Dead.
2. OWNS: owned objects are included in D (transitive).
3. DEPENDS_ON: for edges to in D, emit DependencyInvalidated(dependent=from, dependency=to); dependents stay Alive unless also in D.
4. REFERENCES: edges removed; no notification.
5. Topology: edges incident to D removed after semantic handling.
6. Capability: no requirement to eagerly purge graph; use-time check requires resource Alive (and validity/holder rules as specified).

## Links

- Must carry LinkType.
- Link/unlink transactional.
- Same-direction same-type uniqueness as implemented policy.

## Non-guarantees

- Death Dispatcher module (optional refactor).
- Instant physical deletion of all capability records on death.
