# VERITAS STRENGTH TEST REPORT

Date: 2026-08-13  
Branch: origin/main (dfb0027 + local strength suite)  
Principle: tests only — no production code changes

---

## A. Existing Coverage Audit (Gap Analysis Summary)

### What was already strong

| Area | Coverage | Notes |
|------|----------|-------|
| Multi-object transaction matrix | 21 tests | P3 sealed: grant/write/link/abort/WAL |
| Capability grant/delegate/revoke | Unit + recovery | Graph cascade, identity, regrant |
| WAL recovery happy path | ~32 tests | Equivalence, invariants, object, robustness |
| WAL truncate/corrupt (basic) | 7 robustness | last N bytes, single-byte, empty, idempotent |
| Snapshot isolation / OCC | transaction/* | read-own-writes, conflict, abort rollback |
| Object lifecycle | object/* | birth/freeze/death/OWNS cascade |
| Checkpoint/snapshot | checkpoint_* | roundtrip + continuity |
| Machine E2E | machine/basic | CALL identity switching |
| world_demo | 1 E2E | multi-object closed loop |

### Gaps identified (pre-strength suite)

| ID | Gap | Severity |
|----|-----|----------|
| G1 | Illegal grantor / identity spoofing via WorldService | High |
| G2 | Self-access exemption boundary on unrelated objects | High |
| G3 | Cross-session pending capability leakage | High |
| G4 | Post-abort / post-commit session reuse | Medium |
| G5 | WAL multi-offset truncation + systematic byte flips | Medium |
| G6 | Recovery N-times idempotence on complex worlds | Medium |
| G7 | Duplicate WAL line behavior | Medium |
| G8 | Concurrent multi-session (different/same object) | Medium |
| G9 | Stress 100 / 1000 objects + recovery consistency | Medium |
| G10 | Empty / large payload / overwrite semantics | Low |
| G11 | ObjectId 0 / nonexistent object ops | Low |
| G12 | Link without target capability (observed open) | Medium |
| G13 | tx_begin with nonexistent actor | Medium |
| G14 | Version counter after recovery appears 0 in logs | Debt |

---

## B. Strength Test Matrix

| ID | Name | Model | Prior cover? | Added? | Expected | Key invariant |
|----|------|-------|--------------|--------|----------|---------------|
| S-A01 | illegal grantor | Malicious identity | Partial | Yes | Reject | No residual cap |
| S-A02 | grantor/grantee swap | Identity | No | Yes | Reject | Permission boundary |
| S-A03 | cross-object write w/o cap | Privilege | Partial | Yes | Reject + root stable | state_root unchanged |
| S-A04 | self-access exemption | Exemption abuse | Partial | Yes | Reject foreign; allow self | exemption scoped |
| S-A05 | abort invalidates pending cap | Lifecycle | Partial | Yes | Later use denied | No active residual |
| S-A06 | revoke then use | Revoke | Yes unit | Yes | holds=false | Graph consistent |
| S-A07 | unauthorized freeze | Privilege | Partial | Yes | Reject; Alive | Lifecycle |
| S-A08 | unauthorized death | Privilege | Partial | Yes | Reject; Alive | Lifecycle |
| S-A09 | link without cap | Privilege | Partial | Yes | Record actual | Documented |
| S-A10 | grantor ≠ holder | Semantics | Yes matrix | Yes | B holds; B writes A | Grant semantics |
| S-B01 | uncommitted isolation | Session | Partial | Yes | No leak | Snapshot |
| S-B02 | pending cap isolation | Session | Partial | Yes | Deny | Session boundary |
| S-B03 | abort clears pending objects | Session | Yes matrix | Yes | Absent | Rollback |
| S-B04 | ended session ops | State machine | No | Yes | NoSession | No panic |
| S-B05 | nonexistent session | State machine | No | Yes | NoSession | No panic |
| S-W01 | truncate multi-offset | Fault inject | Basic | Yes | No panic | Safe recovery |
| S-W02 | single-byte corruption | Fault inject | Basic | Yes | No panic | Safe recovery |
| S-W03 | empty WAL | Fault inject | Yes | Yes | Empty world | Boot |
| S-W04 | duplicate WAL line | Fault inject | No | Yes | Bounded objects | No explosion |
| S-R01 | recovery ×5 idempotent | Replay | Partial | Yes | Same ids/root | Idempotence |
| S-R02 | complex world recovery | Replay | Partial | Yes | Stable ids/links/caps/root | Consistency |
| S-G01 | nonexistent ObjectId | Boundary | No | Yes | ObjectNotFound/err | Clean error |
| S-G02 | ObjectId 0 | Boundary | No | Yes | Birth ≠ 0; ops fail | Identity |
| S-H01 | empty + 64KiB payload | Data | No | Yes | Roundtrip | Memory |
| S-H02 | multi overwrite | Data | No | Yes | Last wins | Write set |
| S-T01–T05 | double commit/abort/… | State machine | Partial | Yes | NoSession / multi-session ok | Deterministic |
| S-E01 | 100 objects | Stress L1 | No | Yes | 100 + recovery | Stability |
| S-E02 | 200 writes one tx | Stress L1 | No | Yes | Commit | Stability |
| S-E03 | 1000 objects | Stress L2 | No | Yes | 1000 + recovery | Stability |
| S-E04 | wide capability graph | Stress | No | Yes | Caps present | Graph |
| S-C01 | concurrent different objs | Concurrency | No | Yes | Both commit | No panic |
| S-C02 | concurrent session lifecycle | Concurrency | No | Yes | 8 objects | No panic |
| S-C03 | concurrent same-object | Concurrency | No | Yes | No panic/deadlock | Progress |

---

## C. New Tests

- **File**: `tests/strength_adversarial.rs`
- **Count**: 37 tests
- **Production changes**: none

---

## D. Results

```
cargo test --test strength_adversarial
→ 37 passed; 0 failed

cargo test (full suite)
→ 286 passed; 0 failed
```

### Stress timings (debug build, this environment)

| Test | Scale | Time |
|------|-------|------|
| s_e01 | 100 objects | ~49 ms |
| s_e02 | 200 writes / 1 tx | ~1 ms |
| s_e03 | 1000 objects + recovery | ~2.0 s |

### Classification of outcomes

| Result | Count |
|--------|-------|
| PASS | 37 |
| FAIL | 0 |
| EXPECTED FAILURE | 0 |
| UNSUPPORTED (documented) | 0 |
| OBSERVED OPEN BEHAVIOR | 1 (S-A09 link) |

---

## E / F / G. Findings

### Finding 1 — Link without explicit target capability may be allowed

- **Test**: `s_a09_link_without_capability`
- **Observed**: `tx_link(A→B, REFERENCES)` from session actor A succeeded without A holding a capability on B (A and B born in separate sessions/commits).
- **Classification**: **④ Known / architecture observation** (or latent policy gap). Existing `machine_object_link_security` covers Machine path; WorldService `tx_link` may not run the same authorize_intent on target.
- **Locate**: `WorldService::tx_link` → `KernelCall::ObjectLink` — verify whether target authorization is enforced before link insertion.
- **Production fix this round**: **No** (test-only policy; recorded for next round).
- **Minimal repro**: see `s_a09` in strength suite.

### Finding 2 — Byte-corrupted WAL → “starting fresh”

- **Test**: `s_w02`
- **Observed**: On UTF-8 / parse failure, recovery logs `WAL recovery failed: stream did not contain valid UTF-8, starting fresh` and boots empty world.
- **Classification**: **③ Architecture-supported boundary** — fail-closed to empty rather than panic. Data loss under corruption is expected with current design (no checkpoint fallback in this path).
- **Locate**: `Kernel::with_wal_path` recovery error path.
- **Fix this round**: No.

### Finding 3 — Recovery log shows `当前版本号: 0`

- **Observed** during stress recovery: version printed as 0 while next_tx_id advances.
- **Classification**: **④ Architecture debt / logging** — needs audit of whether global_version is restored from TransactionCommitted records.
- **Impact**: If version truly resets, OCC / receipts_since may misbehave across process restart.
- **Fix this round**: No (out of scope; flag for next).

### Finding 4 — Session ops serialized by Mutex

- **Observed**: Concurrent tests pass; `sessions: Mutex<HashMap<...>>` and engine field Mutexes mean WorldService is multi-session but coarse-locked.
- **Classification**: **③ By design** — not a bug. Concurrent same-object writes do not deadlock in these tests.

### No panics, no deadlocks, no state corruption detected under the executed matrix.

---

## H. Production fixes this round

**None.** Default principle held: only tests.

---

## I. Real security / stability boundary (as proven)

1. **Capability / identity**
   - Illegal grantor, cross-object write/freeze/death without authority → rejected.
   - Self-access exemption does not extend to unrelated objects.
   - Abort clears pending objects and pending capabilities for later sessions.
   - Revoke clears `holds`.

2. **Session**
   - Ended / nonexistent session → `NoSession`, no panic.
   - Pending capability is session-local (other session cannot use it).
   - Multiple concurrent sessions supported under Mutex serialization.

3. **WAL**
   - Truncation / single-byte corruption / empty / duplicate line → no panic.
   - Corruption may discard WAL and start empty (documented).
   - Multi-recovery on valid WAL is idempotent for object set, links, caps, state_root (complex world included).

4. **Stress**
   - 1000 sequential object births + recovery consistent in ~2s (debug).
   - 200 writes in one tx stable.

5. **Open edges**
   - WorldService link authorization vs Machine path may differ (S-A09).
   - Global version after recovery needs deeper audit.
   - Full u64 ObjectId exhaustion not exercised (unsafe at scale).
   - No deep “mid-record CRC + partial commit marker” differential beyond existing robustness + S-W*.

---

## J. Next-round suggestions

1. **Authorize `tx_link` target** the same way as write/freeze/death (if constitution requires).
2. **Audit `global_version` restore** from `TransactionCommitted` — fix if truly 0 after recovery.
3. **Stronger WAL fault model**: mid-payload CRC-valid-looking garbage; commit marker without full delta; interleave legacy Commit + TransactionCommitted.
4. **Replay-as-attack**: feed an already-applied TransactionCommitted delta into a live engine apply path if exposed.
5. **Deeper concurrent same-object OCC**: assert conflict error type, not only “no panic”.
6. **Level-3 stress** only after Level-2 baselines on release builds with RSS monitoring.
7. Keep Forge unmodified; any WorldAdapter assumptions should track WorldService authorization parity.

---

## Counts (final)

| Metric | Value |
|--------|-------|
| Prior ~tests (list) | ~249 listed + unit |
| Full suite after strength | **286 passed** |
| New strength tests | **37** |
| New test files | **1** (`strength_adversarial.rs`) |
| Attack tests (A) | 10 |
| Session isolation (B) | 5 |
| WAL fault injection (W) | 4 |
| Replay/recovery (R) | 2 |
| Concurrency (C) | 3 |
| Stress (E) | 4 |
| Boundary G/H | 4 |
| Tx state machine (T) | 5 |

**Answer to the question: Veritas 抗不抗打？**

Under this adversarial matrix: **yes for the exercised surfaces** — no panic, no cross-session pending-cap leak, no illegal grantor success, recovery idempotent on valid WAL, stress stable to 1k objects.  
**Open**: link authorization parity (S-A09), version-after-recovery logging/behavior, and deeper CRC/structural WAL attacks remain for the next round — without weakening tests to force green.
