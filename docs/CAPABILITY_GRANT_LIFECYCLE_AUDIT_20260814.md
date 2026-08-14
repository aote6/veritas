# P0 Capability Grant / Capability Lifecycle Decision Audit

**Date:** 2026-08-14  
**HEAD:** `521a96c` (docs: P0 identity drift resolved by architectural decision)  
**Scope:** Architecture boundary / decision audit only.  
**Constraints honored:** No modifications to `src/`, `tests/`, or Constitution.  
**This report is the sole deliverable of this round.**

---

## 1. Executive Summary

Veritas Kernel **already implements a closed Capability lifecycle at the graph + Engine + WAL/Checkpoint layer**:

| Stage | Status in Kernel |
|-------|------------------|
| Create (root GRANT / ObjectBirth AdminCap) | Implemented |
| Attach (`ctx.capabilities` transaction-scoped) | Implemented |
| Grant / Delegate | Implemented (Engine + KernelCall) |
| Use (`authorize_intent` / `holds`) | Implemented |
| Revoke (+ cascade) | Implemented |
| Holder death → `revoke_holder` purge | Implemented |
| Checkpoint / Recovery / Replay | Implemented (stable CapabilityId + graph) |

**What is not closed:**

1. **Grant authorization vs Constitution** — Constitution `kernel.md §3.5` requires *grantor holds AdminCap on resource*. Engine `capability_grant` does **not** enforce this; only checks grantee is Alive. WorldService only authorizes identity switch to grantor (`AccessIntent::Call(grantor)`), not AdminCap-on-resource.
2. **Host surface asymmetry** — `WorldService.tx_capability_grant` + `veritasd` JSONL command exist. **No** `tx_capability_revoke` / `tx_capability_delegate` on WorldService or veritasd.
3. **VASM instruction surface** — Only `Instruction::CapabilityGrant` exists. No Revoke/Delegate opcodes.
4. **TRAP decode gap** — `KernelCall::decode` does not map service_ids for CapabilityGrant/Revoke/Delegate (enum + `handle()` support them; TRAP path does not).
5. **Constitution model drift** — Spec describes Capability tree with `parent: Option<CapabilityId>`; implementation is a **forest of holders under one CapabilityId** with `DelegationEdge` (functionally equivalent for cascade, different shape).

**Overall verdict class: B — semantic design / authorization gap on GRANT, plus API exposure gaps on Revoke/Delegate.**  
Not class C (no confirmed “attacker obtains capability without any production path control” when Host is trusted and only uses WorldService after identity auth — but Machine-native GRANT can invent root caps on foreign resources).

**本轮是否需要修改 Kernel？ → 需要语义设计**（Grant 授权谓词 + Host 暴露策略；非立即安全热修，也非仅 API 暴露）。

---

## 2. Capability Model — Code Facts

### 2.1 What is a Capability?

From `src/capability.rs` + `src/types.rs`:

- Capability is a **Kernel-managed resource**, not an Object (matches Constitution).
- Identity: `CapabilityId` (u64), deterministic:
  ```text
  capability_id_of(grantor, grantee, resource, grant_sequence)
  = deterministic_hash("__cap__:{grantor}:{grantee}:{resource}:{grant_sequence}")
  ```
- `grant_sequence` is **CapabilityGraph-local** monotonic counter (not tx_id / global_version), so grant→rollback→regrant does not collide.
- `CapabilityInfo`: `{ capability_type, granted_by, root_holder, resource }`
- Holders: `HashMap<(CapabilityId, ObjectId), HolderRecord { active, parent }>`
- Delegation: forest under one `capability_id` (not a DAG of CapabilityIds).

### 2.2 CapabilityId stability

| Event | CapabilityId behavior |
|-------|------------------------|
| Fresh grant | New id from (grantor, grantee, resource, seq) |
| Same tuple after revoke + regrant | **Different** id (seq advanced) — unit-tested |
| Checkpoint restore | **Persisted** id restored via `restore_capabilities` / `restore_grant` — **never** recomputed with `capability_id_of` on restore |
| WAL replay | Delta carries `capability_id` + `grant_sequence`; `apply()` uses `restore_grant` |

**Fact:** CapabilityId is stable across checkpoint and WAL recovery when records are persisted correctly.

### 2.3 Owner / holder / grantor / grantee

| Role | Meaning in code |
|------|-----------------|
| **grantor** (`granted_by`) | Object recorded as issuer of the **root** grant |
| **grantee / root_holder** | First holder of the root grant (`parent: None`) |
| **holder** | Any `(cap_id, object)` with a HolderRecord |
| **parent holder** | `HolderRecord.parent: Option<ObjectId>` — upstream **holder**, not parent CapabilityId |
| **resource** | ObjectId the capability authorizes access to |

There is no separate “owner” field beyond `granted_by` / `root_holder`.

### 2.4 Two layers that must not be collapsed

| Layer | Storage | Lifetime | Role |
|-------|---------|----------|------|
| **Persistent graph** | `Engine.capability_graph` | World lifetime; checkpoint + WAL | Truth for `holds()`, cascade, recovery |
| **Transaction attach list** | `ctx.capabilities: Vec<u64>` | Single transaction | One input to `authorize_intent` |

`attach_capability` only does `ctx.capabilities.push(cap_id)`.  
It does **not** mean “Object permanently owns Capability”.

`authorize_intent` accepts access if **any** of:

1. `target == current_object || target == capability_context` (self-access exemption)
2. Attached cap still actively held on target resource (`has_committed`)
3. Pending grant in this tx covers target (`has_pending`)
4. Live graph: current_object or capability_context actively holds any cap on target (`has_graph`)

So persistence is the graph; attach is transactional sugar + same-tx path.

---

## 3. Capability Creation / Birth

### Call chain A — ObjectBirth → AdminCap → commit → WAL → recovery

```
ObjectBirth (Machine / KernelCall / WorldService.tx_create_object)
  → engine.object_birth(ctx, object_id)
      → pending_objects.push(object_id)
      → pending_capabilities.push(self-AdminCap:
            grantor=object_id, grantee=object_id, resource=object_id, type="AdminCap")
      → if current_object != object_id && current_object != 0:
            pending_capabilities.push(creator-AdminCap:
              grantor=object_id, grantee=current_object, resource=object_id)
  → [Machine only] attach self-AdminCap into ctx.capabilities (explicit, no identity switch)
  → commit → verify_capability → build_delta(capability_grants) → WAL TransactionCommitted
  → apply() → cap_graph.restore_grant(...)
  → checkpoint: snapshot_capabilities() includes holders + active + parent + cascade_on_revoke
  → recovery: set_grant_sequence + restore_capabilities
```

**Formal semantics of self-AdminCap at birth:**

- It is a **root GRANT** recorded as `PendingCapabilityGrant`, not a separate “attach-only” concept.
- Creator AdminCap is a **second root grant** (different CapabilityId) when creator ≠ newborn and creator ≠ 0.
- Host bootstrap (`current_object == 0`) does **not** get creator AdminCap (intentional exception, aligned with Identity P0).

**self-AdminCap is creation (grant into pending graph state), not merely attach.**  
Attach is an additional Machine same-tx convenience so CALL can pass `authorize_intent` before commit.

---

## 4. Capability Attach Semantics

| Question | Answer from code |
|----------|------------------|
| Attach where? | **TransactionContext only** (`ctx.capabilities`) |
| Object-embedded capability list? | **No** — Constitution `capability_space` is “indexed by holder from graph”, not object-inline |
| Kernel global? | Graph is global; **attach list is not** |
| Survive commit? | Attach list is **not** written to WorldState; only pending grants/delegates/revokes become graph mutations |

**Consistency with persistent identity:**  
After commit, use-path relies on `has_graph` / `holds_capability`, not on `ctx.capabilities`.  
Attach is transaction-scoped; persistent identity is CapabilityId + HolderRecord.

---

## 5. Capability Grant / Delegation

### Call chain B — Grant / Delegate

```
Path 1 — WorldService (Host):
  veritasd "tx_capability_grant" 
    → WorldService.tx_capability_grant(session, grantor, grantee, type, resource)
      → if current_object != grantor: authorize_intent(Call(grantor)); enter_object(grantor)
      → KernelCall::CapabilityGrant { grantor, grantee, capability_type, resource }
        → engine.capability_grant
          → ObjectGuard::ensure_can_grant(grantee)  // Alive only
          → compute seq / capability_id
          → pending_capabilities.push(...)
      → commit → WAL → apply restore_grant

Path 2 — Machine VASM:
  Instruction::CapabilityGrant { holder, permission, resource }
    → grantor = ctx.current_object
    → KernelCall::CapabilityGrant → same engine path

Path 3 — Tests / internal:
  Kernel.handle(CapabilityGrant | CapabilityDelegate | CapabilityRevoke)
```

**Delegate:**

- Engine `capability_delegate`: requires `from` holds (graph or same-tx pending grant/delegate); forest constraint on `to`; no new CapabilityId; `pending_delegates` → apply `cap_graph.delegate`.
- **No** WorldService / veritasd / VASM opcode surface.

**Who can Grant (code fact vs Constitution):**

| Check | Constitution | Code |
|-------|--------------|------|
| grantee Alive | implied | **Yes** (`ensure_can_grant`) |
| grantor holds AdminCap on resource | **Required §3.5** | **No** |
| Actor may act as grantor | (Host policy) | WorldService: Call(grantor) if switch needed |
| Machine grantor | — | Always `current_object`; no AdminCap-on-resource |

**authorize_intent is used for identity switch on Host grant path, not for “permission to mint a root grant on resource”.**

### Decision 1 — Capability Grant

**SEMANTIC GAP** (authorization predicate missing relative to Constitution), with **Host API present**.

- Not “only API exposure missing” — grant **exists** end-to-end.
- Not classified as confirmed production **Security Bug** under the Identity-P0 discipline (Host path still requires acting as grantor; self-access and graph use remain gated). However, **any object can mint a new root capability on an arbitrary resource** via GRANT if it is current_object — this **violates the written Constitution** and weakens the intended capability monopoly on cross-object authority.

---

## 6. Capability Revoke

### Call chain C

```
KernelCall::CapabilityRevoke { capability_id, holder, cascade_override }
  → engine.capability_revoke
      → if same-tx pending grant matching (id, grantee=holder): remove pending (+ related delegates)
      → else require cap_graph.holds(id, holder)
      → pending_capability_revokes.push
  → commit → WAL delta.capability_revokes
  → apply → cap_graph.revoke(id, holder, cascade_override)
       → flip holder.active=false; if cascade, deactivate_subtree
```

- Revoke is **soft** (active flag); does not delete grant identity (except Death path).
- **No** WorldService / veritasd / Machine instruction.
- Tests: `tests/capability_revoke.rs` closes Kernel→Engine→Graph→WAL/Checkpoint via KernelCall.

### Decision 2 — Capability Revoke

**IMPLEMENTATION COMPLETE at Kernel; API EXPOSURE GAP at WorldService/veritasd/VASM.**  
Also **DOCUMENTATION GAP** if Host operators assume revoke is only via Object death.

---

## 7. Cascade Revocation

Implemented in `CapabilityGraph::revoke` + `deactivate_subtree`:

- `cascade_override: Some(bool)` forces behavior.
- `None` → use incoming edge’s `cascade_on_revoke`; root defaults **true**.
- Non-cascade: holder deactivated; downstream holders remain active if their edges said so.
- Unit tests cover cascade, non-cascade, override, root default.

Death path uses **different** mechanism: `revoke_holder` → `purge_subtree_strictly` (physical remove of holder/edges/grants when empty) — not the soft revoke cascade.

### Decision 3 — Cascade Revocation

**COMPLETE** at graph + Engine + WAL apply.  
Host exposure missing (same as Revoke).

---

## 8. Capability Graph

- Forest per `capability_id`; anti-cycle via `AlreadyInTree`.
- Edges retained after soft revoke for audit; graph grows over time (documented prototype tradeoff).
- Snapshot: `CapabilitySemanticRecord` includes `capability_id`, `granted_by`, `holder`, `resource`, `capability_type`, `active`, `parent`, `cascade_on_revoke`.
- Parent/child graph **is** persisted in checkpoint and reconstructed on restore.

**Lazy death invalidation (resource):**

- Constitution: resource death → capabilities on that resource auto-invalid (lazy at use).
- Code: **holder** death → eager `revoke_holder` purge at apply.
- `authorize_intent` does **not** check whether `resource` Object is Dead.
- Caps whose resource is dead may still appear held until something else removes them.

**Classification:** partial model — holder death closed; resource-liveness-at-use is **semantic/documentation drift** vs Constitution §7 / object.md (not proven as active exploit in Host path without further use of dead resources).

---

## 9. Checkpoint / WAL / Recovery / Replay

| Mechanism | Capability coverage |
|-----------|---------------------|
| `TransactionDelta` | `capability_grants`, `capability_delegates`, `capability_revokes` + codecs |
| `apply()` order | grants → delegates → revokes → … → death revoke_holder |
| `WorldSnapshot` | `capability_records` + `grant_sequence` |
| `restore_checkpoint` | `set_grant_sequence` then `restore_capabilities` |
| WAL legacy entries | CapabilityGrant entries still parseable; primary path is TransactionCommitted delta |

**CapabilityId after recovery:** stable (persisted id).  
**Graph after replay:** should match pre-crash if deltas applied in order; tests exist (`capability_revoke`, `capability_delegate_p4_recovery`, `checkpoint_roundtrip`, wal recovery suites).

**Permission drift risk:** soft-revoked holders stay inactive after restore (`active` in snapshot).  
Replay ignoring revoke errors (`let _ = cap_graph.revoke`) is intentional for edge cases; commit path validates.

### Decision 4 — Capability Persistence / Recovery

**COMPLETE** for grant/delegate/revoke records + grant_sequence + CapabilityId stability.  
Residual: resource-dead lazy invalidation not mirrored in authorize path.

---

## 10. authorize_intent / AccessIntent Security Audit

**Production cross-object Host paths** (from Identity P0 + this audit):

- `tx_write` / freeze / death / grant (when switching): `authorize_intent` **before** `enter_object`.
- Machine CALL: authorize then CallFrame switch.

**Grant minting path:** does **not** call authorize_intent on **resource**.

**No production path found that attaches or commits a capability without going through `pending_capabilities` / graph APIs** — bootstrap exceptions are ObjectBirth AdminCap and Host session `current_object==0` (no creator cap).

**Implicit capability:** none beyond self-access exemption (structural, not graph-resident) — intentional, documented in Constitution §7.1.

**Dangerous fallback:** Engine GRANT without AdminCap-on-resource is the primary concern (see §5).

---

## 11. WorldService / veritasd / Forge Surface

| Operation | Kernel | Machine insn | WorldService | veritasd JSONL |
|-----------|--------|--------------|--------------|----------------|
| Grant | Yes | Yes | `tx_capability_grant` | `tx_capability_grant` |
| Delegate | Yes | **No** | **No** | **No** |
| Revoke | Yes | **No** | **No** | **No** |
| Birth AdminCap | Yes | Yes | via create_object | via create |

**Forge / WRI:** ROADMAP claims P0 CapabilityGrant e2e closed; this audit does not re-run Forge. Kernel facts: **legal grant is expressible** via WorldService. **Legal revoke/delegate are not expressible** via Host JSONL without raw KernelCall (tests-only).

### Decision 5 — Forge / veritasd Capability Surface

**API EXPOSURE GAP** for Revoke/Delegate.  
**SEMANTIC GAP** for Grant authorization predicate (even though command exists).

---

## 12. Dead Code / Legacy / TODO Classification

| Item | Classification |
|------|----------------|
| `KernelCall::decode` missing Cap Grant/Revoke/Delegate service_ids | **API / ABI gap** — enum+handle live; TRAP decode incomplete (not dead) |
| `Instruction` only CapabilityGrant | **Surface gap**, not dead code |
| `load_snapshot_grants` / older grant-only snapshot | **Legacy helper**; `restore_capabilities` is current path |
| `revoke_holder` physical purge vs soft `revoke` | **Two intentional paths** (Death vs CAPABILITY_REVOKE) |
| Constitution parent=CapabilityId vs code parent=ObjectId | **Documentation / model drift** |
| ROADMAP “P0 CapabilityGrant 闭环 ✅” | **Partially true** for Host grant path; overclaims full lifecycle closure |
| MemoryAlloc KernelCall stub | Unrelated; returns Success without work |

No pure “dead grant API” found — grant path is live. Revoke/Delegate are live at Kernel, **unexposed** at Host.

### Decision 6 — Dead Code / Legacy

- No grant/revoke/delegate **dead code** that should be deleted as meaningless.
- Incomplete TRAP decode and missing Host/VASM surfaces are **exposure gaps**, not corpses.
- Dual revoke semantics (soft vs death purge) are **meaningful**, not accidental.

---

## 13. Security Findings

| ID | Finding | Class |
|----|---------|-------|
| S1 | `capability_grant` does not require grantor to hold AdminCap (or any cap) on `resource` | **Semantic gap / Constitution violation**; security-relevant if Machine or Host callers are untrusted |
| S2 | WorldService grant only gates **identity** (Call grantor), not **resource authority** | Same as S1 for Host when `current_object == grantor` |
| S3 | No unauthorized Identity bypass found for capability **use** (authorize_intent + graph) beyond known self-exemption | Consistent with Identity P0 |
| S4 | Resource-dead capabilities may still pass `holds` / authorize | **Semantic gap** vs lazy invalidation text |
| S5 | WAL apply swallows delegate/revoke errors | Replay hardening, not live-path bypass |

**No finding of class “attacker obtains Capability without going through grant/birth APIs”.**  
Birth and grant are the only producers; the issue is **who is allowed to call grant**.

---

## 14. Documentation Drift

| Doc | Drift |
|-----|-------|
| `constitution/kernel.md §3.5` | Requires AdminCap for GRANT — **not enforced in Engine** |
| `constitution/kernel.md §7` | Capability tree parent=CapabilityId — **code is holder forest** |
| `constitution/kernel.md §3.6` | Revoke “deletes edge + cascade” — code **soft-deactivates** |
| `constitution/object.md` | capability_space as object concept — implemented as graph index only |
| `ROADMAP_NEXT.md` | “P0 CapabilityGrant 闭环” — Host grant yes; full lifecycle no |
| `ARCHITECTURE_DEBT.md` | Correctly lists grant/delegate/revoke as ACTIVE at Kernel; Host surface detail thin |

---

## 15. Decision Table

| # | Topic | Decision |
|---|--------|----------|
| 1 | Capability Grant | **SEMANTIC GAP** (missing AdminCap-on-resource check) + Host API present |
| 2 | Capability Revoke | **COMPLETE** at Kernel; **API EXPOSURE GAP** Host/VASM |
| 3 | Cascade Revocation | **COMPLETE** at Kernel |
| 4 | Persistence / Recovery | **COMPLETE** (Id + graph + sequence); resource-lazy gap residual |
| 5 | Forge / veritasd surface | **API EXPOSURE GAP** (revoke/delegate); grant command exists |
| 6 | Dead / Legacy | No meaningless dead capability core; ABI/surface gaps only |
| 7 | Overall lifecycle | **B — needs architecture/semantic design** (grant auth) + Host exposure for revoke/delegate |

---

## 16. Forge Impact

- Forge can express **grant** today via veritasd `tx_capability_grant` (per existing ROADMAP/tests narrative).
- Forge **cannot** express revoke/delegate without new JSONL commands or out-of-band Kernel access.
- Any Forge model that assumes “only AdminCap holders can grant on a resource” is **not enforced by Kernel** — product/policy must not rely on that until S1 is closed.

---

## 17. Recommended Next Step

1. **Decision workshop (no code yet):** Fix GRANT authorization predicate:
   - Option A: Engine enforces “grantor holds active AdminCap (or designated grant-cap) on resource” (align Constitution).
   - Option B: Amend Constitution — root GRANT is trusted Host/Machine privilege; only Delegate requires hold.
2. After decision: design Host surface for Revoke/Delegate (or explicitly document KernelCall-only).
3. Optional: VASM opcodes + TRAP service_id mapping for parity.
4. Optional: resource-liveness check in `authorize_intent` / `holds` path.
5. Do **not** reopen Identity CALL/RETURN/BIRTH/LINK frozen semantics.

---

## 18. Final Verdict

**Capability 生命周期在 Kernel 图 / Engine / WAL / Checkpoint 层基本闭合；Grant 的“谁有权对 resource 发根授权”相对 Constitution 未闭合；Revoke/Delegate 缺 Host/VASM 暴露。**

**本轮是否需要修改 Kernel？**

### → **需要语义设计**

**理由：**  
生命周期数据结构与 revoke/cascade/recovery **不是**主要缺口；主要缺口是 **GRANT 授权谓词与 Constitution 不一致**（语义设计选择：Engine 强制 AdminCap vs 修订宪法），以及 Revoke/Delegate 的 **Host 暴露策略**。这不是“只加 API 即可”的纯暴露问题，也不是已确认的可立即热修的单一安全洞（需先冻结语义再改代码）。本轮审计禁止改 `src/`；后续实现必须先完成上述 Decision。

---

## Appendix — Tests

| Item | Status |
|------|--------|
| Historical baseline (prior stage) | `cargo test` **332 passed / 0 failed** at HEAD narrative |
| This round | **Did not run** full `cargo test` — environment has **Cargo.lock version 4** vs **cargo 1.75**, same conflict pattern as prior audits; lock not polluted |
| Code-read verification | capability.rs unit tests, engine grant/revoke/delegate, world_api grant, veritasd grant, recovery/apply paths inspected |

Do not treat this round as having re-verified the 332 count.

---

*End of audit. No src/tests/Constitution changes.*
