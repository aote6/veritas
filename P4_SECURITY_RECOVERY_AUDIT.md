# P4: Veritas Security & Recovery Differential Audit

Date: 2026-08-13  
HEAD: `7c484d8` + local `tests/security_recovery_audit.rs`  
Production code modified: **0**  
Forge modified: **0**

---

## 1. Real architecture (code-traced)

### Link path

```
WorldService::tx_link
  → KernelCall::ObjectLink
  → Engine::object_link  (stages pending_links only)
  → WorldService::tx_commit
  → Kernel::commit → Engine::commit
  → verify_capability
  → collect_access_intents → AccessIntent::Link(from, to)
  → authorize_intent  (targets = [from, to]; self-access exempt)

Machine Instruction::ObjectLink
  → KernelCall::ObjectLink  (NO enter_object; historical bypass fixed)
  → same commit → verify_capability path
```

### Version recovery path

```
Kernel::with_wal_path / Engine::with_wal_path
  → RecoveryManager::recover(path) → (records, max_version)
       max_version only from legacy WalEntry::Commit | Checkpoint
       **does NOT read TransactionCommitted.commit_version**
  → global_version := AtomicU64::new(recovered_version)   // often 0 on modern WAL
  → build_ordered_deltas(records)
  → for delta in ordered_deltas { engine.apply(delta) }
       apply() does: global_version.store(delta.commit_version)
  → eprintln!("当前版本号: {}", recovered_version)  // prints PRE-apply recover() value
```

---

## 2. WorldService / Machine authorization对照表

| Operation | WorldService pre-auth | Commit-time auth | Machine |
|-----------|----------------------|------------------|---------|
| write (cross-object) | Yes (`Call(target)` before switch) | Yes (Write intent) | N/A / via kernel |
| freeze | Yes (`Call(object)`) | Yes (Freeze) | via kernel |
| death | Yes (`Call(object)`) | Yes (Destroy) | via kernel |
| capability_grant | Yes if switching to grantor | Yes | via kernel |
| **link** | **No pre-auth** | **Yes** `AccessIntent::Link(from,to)` | **No pre-auth; commit gates** |
| unlink | No pre-auth | Yes (Unlink from only) | same |

**Answers to Part 1 questions:**

1. **source (`from`)** — authorized at commit if ≠ current_object / capability_context (needs cap), else self-exempt.  
2. **target (`to`)** — same rule; typically the one that fails without AdminCap on B.  
3. **Intent** — `AccessIntent::Link(from, to)` → both endpoints.  
4. **WorldService vs Machine** — **parity on commit outcome** (both reject unauthorized link). Difference is only API surface: freeze/death/write pre-check at WorldService; link does not.  
5. **Not a security bypass** for link: staging succeeds, **commit rejects**. Prior strength S-A09 “OBSERVED allowed” was a **weak assertion** (checked stage, not commit).

---

## 3. global_version recovery — real conclusion

| Check | Result |
|-------|--------|
| `get_global_version()` after recovery | **Correct** — equals last `delta.commit_version` |
| `state_root` / object set | **Correct** |
| New tx `snapshot_version()` after recovery | **Equals** restored global_version |
| Post-recovery commit version | **Continues** `v → v+1` |
| `receipts_since` after recovery | **Sees** historical versions |
| Log `当前版本号: 0` | **Misleading** — prints `recover()` max_version, not post-apply engine version |
| `RecoveryManager::recover` max_version | **Ignores** `TransactionCommitted` (only legacy Commit/Checkpoint) |

**Classification:** ⑤ log / recover()-API incompleteness + ③ architecture still safe for OCC via apply().  
**Not** a live OCC semantic bug under normal recovery.  
**Root cause location:** `src/wal.rs` `RecoveryManager::recover` match arms (~L516); log in `src/engine.rs` `with_wal_path` (~L816).

---

## 4. WAL attack matrix

| Attack | Panic? | Silent corruption? | Objects | Links | Caps | Root | Version | Classification |
|--------|--------|--------------------|---------|-------|------|------|---------|----------------|
| Duplicate TXCOMMIT lines | No | Possible mild drift | Bounded | — | — | stable enough | stable | ⑤ fail-soft |
| Duplicate birth same id | No | Registry overwrite possible | id present | — | — | — | advances | ③ boundary |
| Out-of-order lower version TXCOMMIT | No | **Version regression** | may add births | — | — | may change | **regresses** | **① true gap** |
| Duplicate link via append | No | **Edge count can be 2** | — | duplicate edge | — | — | advances | **① mild** / ③ |
| Duplicate cap grant append | No | active count stayed 1 | — | — | no explode | — | advances | ⑤ mostly idempotent |
| Empty delta version bump | No | version jumps | unchanged | — | — | — | jumps | ③ no validation |
| BIRTH id=0 | No | object 0 may appear | includes 0 | — | — | — | set | ③ weak validation |
| Bad CRC trailing line | No | stops further reads | prior preserved | — | — | — | prior | ⑤ fail-safe |
| Multi recovery idempotent | No | No | same | same | same | same | same | PASS |
| recovery→commit→recovery chain | No | No | consistent | — | — | — | monotonic | PASS |

---

## 5. Replay attack

- Re-opening the same WAL N times → identical `WorldSnap` (ids, links, caps, root, version).  
- **Idempotent** for clean WAL recovery.  
- No public API to inject an already-applied delta into a live engine without appending WAL; internal `apply` is `pub(crate)`.  
- Recorded as: **current attack surface = WAL file append + recovery**, not live double-apply API.

---

## 6. Findings (precise)

### Finding A — S-A09 resolved as false positive on security

- **Layer:** WorldService stage vs Engine commit  
- **Fact:** `tx_link` stages; `tx_commit` returns `PermissionDenied` without target cap.  
- **Parity:** Kernel/Machine same.  
- **Class:** ② prior test assumption weak (stage ≠ commit success).  
- **True bug?** **No** for authorization outcome.

### Finding B — recover() max_version ignores TransactionCommitted

- **Layer:** WAL Recovery  
- **Function:** `RecoveryManager::recover`  
- **Expected:** max version across all committed forms  
- **Actual:** only legacy `Commit`/`Checkpoint`; modern WAL → 0  
- **Engine after apply:** correct  
- **Class:** ⑤ log + incomplete recover() return value; **not** OCC bug if callers use `get_global_version()` after construction  
- **True production bug for OCC?** **No** under normal path. **Yes** if any caller trusts `recover()`’s returned version alone.

### Finding C — Out-of-order / lower commit_version accepted

- **Layer:** Recovery apply  
- **Function:** `VeritasEngine::apply` stores `delta.commit_version` unconditionally  
- **Expected (security/consistency):** reject or ignore version regression  
- **Actual:** `global_version` can go `2 → 1`; extra births applied  
- **Class:** **① real recovery validation gap** (adversarial WAL / append attack)  
- **Minimal fix locus (not applied this round):** validate monotonic version in `build_ordered_deltas` or `apply`, or take `max` instead of last store  

### Finding D — Duplicate link edges after double TXCOMMIT

- **Observed:** A→B edge count = 2 after append of identical link delta  
- **Class:** **① mild topology hygiene gap** (or ③ if multi-edges intentional — unlikely)  
- **Locate:** `apply` link insertion without dedup  

### Finding E — ObjectId 0 birth via crafted WAL

- **Observed:** `ids=[0,1]` after BIRTH 0  
- **Class:** ③ weak validation on recovery apply  

---

## 7. Classification summary

| # | Finding | Class |
|---|---------|-------|
| A | Link “allowed” without cap | ② weak prior assertion — **not a bug** |
| B | Log version 0 | ⑤ log / recover() API — engine OK |
| C | Version regression via WAL | **① true recovery gap** |
| D | Duplicate links | **① mild** |
| E | Birth object 0 | ③ boundary |

---

## 8–11. Counts & results

| Metric | Value |
|--------|-------|
| New file | `tests/security_recovery_audit.rs` |
| New tests | **22** |
| `cargo test --test security_recovery_audit` | **22 passed** |
| Full `cargo test` | **308 passed / 0 failed** |
| Production code changes | **0** |

---

## 这轮到底发现了几个真实生产 bug？

**严格①（真实生产/恢复路径缺陷，可由恶意或损坏 WAL 触发）：2 个**

1. **Recovery 不校验 commit_version 单调性** → 低版本 `TransactionCommitted` 可把 `global_version` 回拨，并应用额外 births（`audit_wal_out_of_order_version`）。  
2. **apply 对 link 不幂等去重** → 重复 TXCOMMIT 可产生重复边（`audit_wal_duplicate_link_records`，edge count=2）。

**非①但需记录：**

- S-A09 / WorldService link：**不是**授权漏洞（commit 拒绝）；与 Machine **parity**。  
- 日志 `当前版本号: 0`：**不是** engine OCC 语义错误；是 `recover()` 未统计 `TransactionCommitted` + 日志打印错误变量。

**本轮未修生产代码。** 建议下一轮最小修复点：

1. `RecoveryManager::recover`：从 `TransactionCommitted` 取 `max(commit_version)`。  
2. 日志打印 **apply 后的** `get_global_version()`。  
3. `apply` 或 `build_ordered_deltas`：拒绝/跳过 `commit_version < current global_version`（或单调合并策略）。  
4. link apply：`(from,to,type)` 去重。
