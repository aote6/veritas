# P0 Identity Drift Decision Audit

**审计日期**: 2026-08-14  
**审计类型**: 架构定界审计（非修复任务）  
**约束**: 禁止修改 `src/`、`tests/`、Constitution；本报告为唯一新增产物。  
**聚焦**: Identity / Session Bootstrap / Object Switching  
**数据来源**: `src/types.rs`、`src/engine.rs`、`src/world_api.rs`、`src/machine.rs`、`src/kernel.rs`、`docs/IDENTITY_MODEL.md`、`docs/constitution/{kernel,transaction,object,world}.md`、`docs/ARCHITECTURE_DEBT.md`、相关 tests、`rg` 调用链检索。

---

## 1. Executive Summary

Veritas 执行期身份模型在 **Machine 路径**上已经钉死：

- `CALL` → `authorize_intent(AccessIntent::Call)` → 同步切换 `current_object` + `capability_context` → `RETURN` 从 `CallFrame` 恢复。
- `OBJECT_BIRTH` **不**切换身份；仅 attach 新生对象的 self-AdminCap，使后续 CALL 可走通审计。
- `OBJECT_LINK` **不**隐式 `enter_object`。

同时存在一套 **Host / Session 层** 身份原语，与 Machine 不同构，且未被 `IDENTITY_MODEL.md` 完整描述：

| 原语 | 层 | 是否经 authorize_intent | 是否切换 capability_context |
|------|-----|-------------------------|-----------------------------|
| `Machine::CALL` / `RETURN` | 执行期 | 是 | 是（双向） |
| `WorldService::tx_begin(actor)` | Session bootstrap | 否（建立会话） | 是（设为 actor） |
| `Engine::begin_in_object` | Session bootstrap | 否 | **否**（仅 current_object） |
| `TransactionContext::enter_object` | 底层原语 | 否（调用方负责） | **否**（仅 current_object） |
| `Machine::set_execution_object` | 测试/引导 | 否 | 是 |
| `tx_create_object` 当 `current_object==0` | Session bootstrap 例外 | 否 | 是（设为新 id） |
| WorldService `tx_write`/`freeze`/`death`/`grant` 跨对象 | Host API | **是**（先 authorize 再 enter） | **否**（只改 current_object） |

**安全结论（本轮）**:

- **未发现**任何生产路径可以在不经 `authorize_intent` 的情况下，从 A 身份变成 B 身份并以 B 的 capability_context 执行特权操作。
- WorldService 跨对象路径均在 `enter_object` **之前**调用 `authorize_intent(AccessIntent::Call(...))`；失败则不切换。
- `tx_create_object(current_object==0)` 的 bootstrap 是 **session 建立期例外**，不是执行期绕过；与 Machine `OBJECT_BIRTH` 不切换身份的不变量 **不冲突**（作用域不同）。
- `IDENTITY_MODEL.md` 与 Constitution（transaction.md）主要描述 Machine 执行期模型，对 Host/Session bootstrap 描述不足 → **文档 drift**，不是运行时权限漏洞。

**五项决策摘要**（详见第 10 节）:

1. `tx_create_object(current_object==0)` → **DOCUMENT AS BOOTSTRAP EXCEPTION**
2. `enter_object` → **KEEP INTERNAL**
3. `begin_in_object` → **OFFICIAL SESSION BOOTSTRAP**
4. `Machine::set_execution_object` → **KEEP TEST/BOOTSTRAP ONLY**
5. `CALL` → **继续作为唯一执行期 identity switch**

---

## 2. 当前身份模型（代码事实）

### 2.1 两个正交概念

| 字段 | 语义 | 主要消费者 |
|------|------|------------|
| `current_object` | 当前操作/地址空间对象（MemorySpace 寻址、读写集 Address） | `engine.read/write`、Address 构造 |
| `capability_context` | capability 授权检查时使用的执行身份 | `authorize_intent`（与 current_object 并列 OR） |

`authorize_intent` 豁免条件：`target == ctx.current_object || target == ctx.capability_context`。

`ctx.capabilities` 是 **transaction-scoped** attach 列表，不是永久 identity-scoped。

### 2.2 两条主路径

**路径 A — Machine 执行期（VASM）**

```
Machine::step
  Instruction::Call
    → authorize_intent(AccessIntent::Call(object_id))
    → push CallFrame { parent_object, caller_capability_context, registers, return_pc }
    → current_object = object_id
    → capability_context = object_id
  Instruction::Return
    → pop CallFrame → 恢复 current_object + capability_context + registers + pc
  Instruction::ObjectBirth
    → KernelCall::ObjectBirth
    → attach self-AdminCap 到 ctx.capabilities
    → **不** enter_object / **不** 改 capability_context
```

**路径 B — WorldService / veritasd Session（Forge 主路径）**

```
tx_begin(actor)
  → begin_in_object(actor) 或 begin()
  → capability_context = actor（若 actor > 0）
  → 挂入 SessionState { ctx, actor }

tx_create_object(sid)
  → ObjectBirth
  → 若 current_object == 0:
       enter_object(id); capability_context = id   // bootstrap only

tx_write / tx_freeze / tx_death / tx_capability_grant（跨对象）
  → authorize_intent(Call(target))   // 必须先于 enter
  → enter_object(target)             // 仅改 current_object
  → 业务 KernelCall / write
```

两条路径 **故意不同构**：Machine CALL 是完整身份栈切换；Host API 是「先授权、再改寻址对象，保留 session actor 作为 capability_context」。

---

## 3. enter_object 调用图

**定义** (`src/types.rs`):

```rust
pub fn enter_object(&mut self, object_id: ObjectId) {
    self.current_object = object_id;
}
```

仅设置 `current_object`，**不**动 `capability_context`。

### 调用者（生产 + 测试入口）

| 调用方 | 文件 | 是否先 authorize_intent | 备注 |
|--------|------|-------------------------|------|
| `Engine::begin_in_object` | engine.rs:910 | 否 | Session bootstrap |
| `Machine::set_execution_object` | machine.rs:168 | 否 | 测试/引导；同时手动设 capability_context |
| `WorldService::tx_create_object` | world_api.rs:335 | 否 | 仅当 `current_object==0` |
| `WorldService::tx_freeze_object` | world_api.rs:356 | **是** | Call(object_id) |
| `WorldService::tx_death_object` | world_api.rs:376 | **是** | Call(object_id) |
| `WorldService::tx_capability_grant` | world_api.rs:421 | **是** | Call(grantor) |
| `WorldService::tx_write` | world_api.rs:469 | **是** | Call(oid) |
| （历史注释）ObjectBirth / ObjectLink | machine.rs | — | 已删除隐式 enter |

**结论**:

- `enter_object` 是 **底层状态设置原语**，不是执行期 identity switch。
- 生产跨对象路径全部 **authorize 后再 enter**；无未经授权的生产绕过。
- 应保留为 **internal primitive**；禁止新业务路径用其替代 CALL 做执行期身份切换。

---

## 4. begin_in_object 调用图

**定义**:

```rust
// engine.rs
pub(crate) fn begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
    let mut ctx = self.begin();
    ctx.enter_object(object_id);
    ctx
}
// kernel.rs 转发；test_api::test_begin_in_object 暴露给测试
```

- **不**设置 `capability_context`（保持默认 0）。
- 调用方若需要完整身份，须自行设 `capability_context`（如 `tx_begin`）。

### 调用者

| 调用方 | 用途 |
|--------|------|
| `WorldService::tx_begin(Some(actor))` | 生产 session bootstrap；随后 `c.capability_context = actor` |
| `tests/**` 大量 `test_begin_in_object` | 集成测试直接开「已在某 object 下」的事务 |
| `kernel.rs` 内部少量测试式用法 | 同左 |

**语义判定（二选一）**:

> **`begin_in_object` = transaction/session bootstrap**，不是执行期身份切换。

证据：

1. 它发生在 Transaction **创建时**，不在 Machine instruction dispatch 中。
2. 不经过 `authorize_intent`，也不维护 CallFrame。
3. 与 `CALL` 不同构：无栈、无 RETURN、不改 capability_context（除非调用方补设）。
4. `tx_begin` 与测试入口明确把它当作「打开一个以某 actor 为当前对象的会话」。

**不应**与 CALL 视为同一种机制。无安全绕过：它不把已有 A 身份「偷换成」B；它是新建 ctx 并初始化。

---

## 5. tx_begin / tx_create_object bootstrap 路径

### 5.1 `tx_begin(actor)`

```rust
let ctx = if actor > 0 {
    // object 必须存在
    let mut c = self.kernel.begin_in_object(actor);
    c.capability_context = actor;
    c
} else {
    self.kernel.begin()  // current_object=0, capability_context=0
};
sessions.insert(sid, SessionState { ctx, actor });
```

- **actor 从哪进入**: 参数 `Option<ObjectId>`，或回落到 `whoami()` / 0。
- **语义**: Session bootstrap，不是执行期切换。一个 `SessionId` 对应一个 `TransactionContext`（存在 `sessions` map 中）。
- 与 Machine CALL **应明确区分**（文档层尚未完全区分 → drift）。

### 5.2 `tx_create_object` 当 `current_object == 0`

```rust
if state.ctx.current_object == 0 {
    state.ctx.enter_object(id);
    state.ctx.capability_context = id;
}
```

**为什么设置**:

- Session 以 `tx_begin(None)` / actor=0 启动时没有寻址对象；随后 `tx_write(..., None)` 依赖 `current_object`。
- 若不进入新生对象，后续 self-write 无法完成（`Address::new(0, state_id)` 语义无意义或被拒绝）。
- 测试锁定行为：`tx_commit_receipt_delta_memory_written`、`session_abort_discards`、`forge_e2e_jsonlines`（`tx_begin` 无 actor → `tx_create_object` → `tx_write`）均依赖此 bootstrap。

**是否与 Machine OBJECT_BIRTH 冲突**:

- **否。** Machine 路径：`OBJECT_BIRTH` 不切换身份，创建者用 CALL 进入。  
- Host 路径：无 VASM 指令流，无 CALL 栈；在 **session 仍无 actor** 时，把「第一个创建的对象」设为 session 工作身份，是 bootstrap 例外，不是执行中途的隐式 CALL。

**是否权限绕过**:

- 无。目标 id 是本 session 刚 birth 的对象；self-AdminCap 已在 ObjectBirth 路径 grant；且仅当 `current_object==0` 才触发。若 session 已有 actor，**保持** creator 身份，不进入 child（与注释一致）。

**Forge 主路径是否依赖**:

- **是。** `tests/forge_e2e_jsonlines.rs`：`tx_begin`（无 actor）→ `tx_create_object` → `tx_write`（无 object_id）→ 依赖 bootstrap 后 `current_object == 新 id`。

---

## 6. CALL / RETURN 正式路径

### 6.1 CALL（唯一执行期 identity switch）

```text
resolve operand → AccessIntent::Call(object_id)
→ authorize_intent(&ctx, &intent)   // 失败 → Trap AccessDenied
→ 可选 pending_calls.push（非 self）
→ push CallFrame {
     return_pc, parent_object=current_object,
     registers, caller_capability_context=capability_context
   }
→ capability_context = object_id
→ current_object = object_id
→ pc = entry_pc
```

- **是**唯一的执行期 identity switch（Machine instruction 层）。
- **是**先 authorize_intent。
- **是**同步切换 current_object + capability_context。

### 6.2 RETURN

从 CallFrame 恢复 `capability_context`、`current_object`、`registers`、`pc`；空栈则 Halt。

### 6.3 相关测试

| 测试 | 锁定内容 |
|------|----------|
| `tests/object_birth_self_call.rs` | OBJECT_BIRTH 不切身份；root 可用 CALL 进入刚 birth 的对象 |
| `tests/machine_object_link_security.rs` | OBJECT_LINK 不隐式 enter；无 cap 必须拒绝 |
| `tests/call_access_intent.rs` | CALL → AccessIntent；set_execution_object 引导 caller |
| `tests/security_recovery_audit.rs` 等 | WorldService 跨对象 authorize 再 enter |
| `tests/multi_object_transaction_matrix.rs` | 连续跨对象写不发生未授权 drift |

---

## 7. 安全绕过检查结果

### 7.1 是否存在「A → 未经 authorize → B 身份 → 以 B 的 capability_context 执行」生产路径？

**未发现未经授权的身份切换生产路径。**

逐条说明：

1. **Machine CALL**: 强制 `authorize_intent`；失败 Trap。
2. **Machine OBJECT_BIRTH / OBJECT_LINK**: 已移除隐式 `enter_object`。
3. **WorldService 跨对象 write/freeze/death/grant**: 均 `authorize_intent` **先于** `enter_object`；且这些路径 **不** 把 `capability_context` 改成目标 B（仍保留 session actor）。即便 enter 成功，后续 authorize 仍以 actor 为主身份之一检查。
4. **tx_begin / begin_in_object**: 新建 ctx，不是从 A 偷换成 B。
5. **tx_create_object(current_object==0)**: 仅 bootstrap 无 actor 的 session 到刚创建的 id；id 由本 session birth，不构成「盗用他人身份」。

### 7.2 `current_object != capability_context` 是否可被利用绕过 authorize_intent？

- WorldService 跨对象成功后会出现 `current_object != capability_context`（仅改了 current_object）。
- `authorize_intent` 对二者做 **OR** 豁免；记忆寻址用 `current_object`。
- 跨对象切换 **已经** 通过 Call 意图审计；后续 self-write 到新 current_object 符合「结构豁免」。
- 未发现「先制造不一致再绕过审计」的路径：失败的 authorize **不** enter，测试 `s_extra_e_consecutive_cross_object_writes_no_drift` 锁定失败后无 sticky 错误上下文。

### 7.3 WorldService bootstrap 是否违反安全模型？

- **不违反运行时安全模型**；它是 Host 层 session 建立语义。
- **违反 / 未写入** 当前 `IDENTITY_MODEL.md` 的狭义表述（「唯一合法切换入口是 CALL」——该表述针对 **执行期**，未覆盖 session bootstrap）。
- 定性：**文档与模型边界 drift**，不是 bug。

### 7.4 begin_in_object 归类

**transaction/session bootstrap**（见第 4 节）。

---

## 8. Forge 影响

- Forge 经 `veritasd` JSONL 使用 WorldService：`tx_begin` → `tx_create_object` → `tx_write` / grant / link / commit。
- **依赖的 identity 行为**:
  1. `tx_begin(None)` → actor=0 session。
  2. `tx_create_object` 在 `current_object==0` 时 bootstrap 进入新对象（否则 write 无寻址对象）。
  3. 跨对象操作走 authorize + enter（仅 current_object）。
- 若 **文档化** bootstrap 语义：**不会**破坏 Forge。
- 若 **删除** `enter_object`：会破坏 WorldService 实现与测试；需先引入替代 API（例如 `set_current_object_after_auth`），属未来重构，非本轮。
- **无需**立刻 API 迁移；优先文档 / 封装层说明即可。

---

## 9. Constitution / IDENTITY_MODEL drift

| 文档 | 表述 | 代码事实 | Drift |
|------|------|----------|-------|
| `IDENTITY_MODEL.md` | 执行期唯一切换入口 = CALL | Machine 路径成立；Host bootstrap 未描述 | **是**（文档不全） |
| `constitution/transaction.md` | CALL 切换 current_object；capability_context 属 Execution Context | 基本一致；未定义 WorldService session | **轻度** |
| `constitution/kernel.md` | self-access 与 ObjectIdentity | 与 authorize 豁免一致 | 无 |
| `ARCHITECTURE_DEBT.md`（同日） | 已标 P0 评估 bootstrap / enter_object | 本审计给出决策 | 关闭评估项 |

**不修改 Constitution**（本轮约束）。建议后续单独 PR：在 IDENTITY_MODEL 增加「Session Bootstrap」一节，明确与 CALL 的边界。

---

## 10. 五项最终 Decision

### Decision 1 — `tx_create_object(current_object == 0)`

**DOCUMENT AS BOOTSTRAP EXCEPTION**

- 保留行为。
- 在 IDENTITY_MODEL / 相关文档中写明：仅当 session 尚无 acting object 时，将首个 birth 对象设为 `current_object` + `capability_context`；已有 actor 时不切换。
- 非 bug；非立即 refactor。

### Decision 2 — `enter_object`

**KEEP INTERNAL**

- 定位：internal primitive（设置 `current_object`）。
- 允许调用者：Engine bootstrap、WorldService 在 **已 authorize** 之后的 Host 路径、Machine 测试引导封装。
- 禁止：新的业务/执行期路径用其替代 CALL 做完整身份切换；禁止在未 authorize 的跨对象生产路径中调用。
- 文档应写明：**不得作为执行期 identity switch 使用**。

### Decision 3 — `begin_in_object`

**OFFICIAL SESSION BOOTSTRAP**

- 正式语义：创建 TransactionContext 并初始化 `current_object`（不自动设 capability_context）。
- 与 CALL 明确区分。
- 保留；测试经 `test_api` 使用合法。

### Decision 4 — `Machine::set_execution_object`

**KEEP TEST/BOOTSTRAP ONLY**

- 同时设置 `current_object` + `capability_context`；用于测试与未来可能的 TRAP 引导。
- 非生产 WorldService 路径；不删除。

### Decision 5 — CALL

**继续作为唯一执行期 identity switch。**

- Machine 指令层：CALL 是唯一经过 authorize 并同步切换 current_object + capability_context、且可由 RETURN 恢复的机制。
- Host/Session bootstrap 不属于「执行期 identity switch」范畴，不削弱此结论。

---

## 11. 推荐的下一步（本轮不执行）

1. **文档**: 扩展 `docs/IDENTITY_MODEL.md`，增加「Session / Host Bootstrap」专节，收录本报告的正式模型（见下）。
2. **注释**: 在 `enter_object`、`begin_in_object`、`tx_create_object` bootstrap 分支加简短定位注释（internal / bootstrap / 禁止当 CALL 用）。
3. **不**删除任何 API；不重开已冻结的安全测试结论。
4. 可选后续（非 P0）：评估 WorldService 跨对象路径是否应同时更新 `capability_context`（当前「只改 current_object」是刻意设计，改动需新测试矩阵）。

### 推荐写入文档的正式模型

```text
执行期（Machine / VASM）:

    CALL
      → authorize_intent(AccessIntent::Call)
      → current_object + capability_context 切换
      → RETURN 从 CallFrame 恢复

Session 建立（WorldService / Host）:

    tx_begin(actor)
      → begin_in_object(actor) 或 begin()
      → 若 actor>0: capability_context = actor
      → 独立 TransactionContext 挂入 Session

Object Birth:

    OBJECT_BIRTH（Machine）
      → 创建 object + grant self-AdminCap
      → attach self-AdminCap 到本事务 ctx
      → 不自动 CALL / 不切换身份

    tx_create_object（Host，current_object==0）
      → ObjectBirth
      → bootstrap: current_object = capability_context = 新 id
      （session 已有 actor 时不切换）

Host 跨对象操作:

    authorize_intent(Call(target))
      → enter_object(target)   // 仅 current_object
      → 业务操作
```

---

## 12. 审计范围与方法说明

- 使用 `rg` 建立 `enter_object` / `begin_in_object` / `set_execution_object` / `capability_context =` / `tx_begin` / `tx_create_object` / `authorize_intent` 的全仓库调用关系。
- 阅读 Machine CALL/RETURN/ObjectBirth、Engine authorize_intent、WorldService session 与跨对象 API 源码。
- 对照 `IDENTITY_MODEL.md`、transaction constitution、ARCHITECTURE_DEBT 同日条目。
- 未修改任何 `src/` 或 `tests/`；未运行格式化或自动修复。

---

*报告结束。本轮唯一允许的代码树变更：新增本文件。*
