# Veritas 执行身份与授权模型说明

最后更新: 2026-08-14

本文档目的：让任何新开的会话（人类或 AI）不需要重新翻整个仓库，
就能知道"身份切换"这个子系统现在是什么状态、为什么是这个状态、
改动一处会牵动哪里。这是之前几轮反复踩坑（同一个错误被"发现→修复
→用错误论证撤销→再次发现"）之后的沉淀，请先读完这份文档再改动
任何跟 `current_object` / capability / enter_object / CALL 相关的代码。

相关审计：
- `docs/IDENTITY_DRIFT_AUDIT_20260814.md`（P0 Identity Drift 定界审计与五项决策）
- `docs/ARCHITECTURE_DEBT.md`（架构债总览；P0 Identity Drift 已文档收口）

---

## 1. 核心不变量（Execution Identity Invariant）

> **`current_object` 的改变只能发生在经过定义和授权的转换点。**
> **普通业务操作不得隐式取得其他 Object 的执行身份。**

在 **Machine / VASM 执行期**，`current_object`（以及同步的 `capability_context`）
唯一合法的完整 identity switch 入口是 **CALL**
（`Instruction::Call`，见 `machine.rs` 的 dispatch）。
切换前必须先通过：

```rust
authorize_intent(&ctx, &AccessIntent::Call(object_id))
```

审计通过才允许 `ctx.current_object = object_id` 且
`ctx.capability_context = object_id`。`RETURN` 从调用栈弹出
`CallFrame` 恢复身份，不需要额外审计（因为切回去的身份本来就是
切换前已经拥有、被记录在 `CallFrame` 里的身份）。

**Host / Session 层**另有 bootstrap 原语（见第 7 节「Session / Host Bootstrap」），
它们创建或初始化执行上下文，**不是**执行期 identity switch，也不得被
当作 CALL 的替代路径使用。

执行期路径上**没有其他例外**。这条规则被违反过两次，两次都以为自己
找到了"安全的特殊情况"，两次都是错的：

- 第一次：`ObjectLink` 用 `enter_object(from)`，理由"程序需要跑通"。
  → 实际是自我授权绕过，commit 时的检查被恒真短路。已删除。
  回归测试：`tests/machine_object_link_security.rs`
- 第二次：`ObjectBirth` 用 `enter_object(id)`，理由"新对象是全新的，
  审计一定会通过所以省略无害"。
  → 实际是这个"一定会通过"的判断从未被验证过，验证后发现
  当时的 CALL 路径根本走不通（capability 发了但没 attach）。
  已删除，改为让 CALL 路径本身变得可行。
  回归测试：`tests/object_birth_self_call.rs`

**如果未来又冒出"这个 case 反正安全，可以走隐式切换"的想法，
先写一个测试，实测走 CALL 是否真的能通过审计。不要凭直觉判断
"审计会通过"，因为两次踩坑都是这个判断错了。**

---

## 2. 谁管什么（模块地图）

```
.vasm 源码
  ↓ assembler.rs (assemble / assemble_module)
    parse_line() 解析每条指令；parse_operand() 解析 Operand；
    labels 支持跳转/CALL目标用符号名
  ↓ instruction.rs
    enum Instruction — 指令的 Rust 表示（新增指令必须先加在这里）
    enum Operand { Immediate(u64), Register(u8) } — 运行时可能变化
    的字段必须用 Operand，不能用裸 u64/ObjectId/StateId
  ↓ instruction_codec.rs
    encode()/decode() — 指令 <-> 字节码。encode_operand/decode_operand
    是 Operand 专用的编解码辅助函数（1字节tag + 8字节值 = 9字节）
  ↓ machine.rs — Machine::step()
    真正执行指令的地方。分两类：
    (a) "本地指令"：Add/Sub/Cmp/Jmp/LoadConst 等纯计算，不碰 kernel
    (b) 需要 kernel 服务的指令：构造 KernelCall，调用
        self.kernel.handle(&mut self.ctx, call)
    dispatch 是穷尽 match（无 `_ => {}` 兜底，编译器强制要求
    每个 Instruction 变体都被处理，这是刻意设计）
  ↓ kernel.rs — Kernel::handle()
    KernelCall 的统一入口，转发到 engine.rs 对应方法
  ↓ engine.rs — VeritasEngine
    真正的业务逻辑：object_birth / object_link / capability_grant /
    authorize_intent / commit 等。这是唯一的事实来源(source of truth)。
  ↓ TransactionContext (types.rs)
    单次事务的可变状态：current_object, capability_context, capabilities,
    pending_capabilities, pending_links, pending_objects 等。
    commit 时把 pending_* 转换成 WalEntry 写入 WAL。

外部 Host 路径（Forge / veritasd）：
  JSONL → WorldService (tx_begin / tx_* / tx_commit)
  → 同一 Kernel / Engine / TransactionContext / WAL
  Session bootstrap 与跨对象路径见第 7 节。
```

---

## 3. `current_object` 的双重身份

`current_object` 同时承担两个角色，这是理解整个模型的关键：

- **A. 当前执行身份**（"我现在是谁在执行代码"）
- **B. 后续操作的默认授权主体**（`authorize_intent` 检查
  `target == ctx.current_object` 时，直接豁免，不查 capability graph）

正因为 B 的存在，"切换 current_object" 这个动作本身就是一次
**隐式的权限升级/降级**——切过去之后，之前需要 capability 才能碰的
东西，现在因为"是自己"而直接免检。这就是为什么"身份切换入口
必须唯一、必须受审计"是一条硬性不变量，不是洁癖：**任何一个能
绕开审计就切换 current_object 的地方，都等价于一个隐藏的权限
提升通道。**

另有 `capability_context`：capability 授权检查时使用的执行身份，
与 `current_object` 在 `authorize_intent` 中并列 OR 豁免。
Machine CALL 同步切换二者；Host 跨对象路径通常只改 `current_object`，
保留 session actor 作为 `capability_context`。

`authorize_intent` 完整判断逻辑（engine.rs）：
对 `intent.target_objects()` 里每个 target：
1. `target == ctx.current_object || target == ctx.capability_context`
   → 直接放行（自己人不用查）
2. 已 commit 的 capability（`ctx.capabilities`）里，是否有一张
   resource 匹配、且当前身份持有的 → 放行
3. 本事务内 pending 的 grant（`ctx.pending_capabilities`）里，
   是否有一条 grantee 匹配当前身份，或者 capability_id 已经
   attach 到 ctx → 放行
4. capability graph 里，当前身份是否持有任意一张 resource 匹配
   的、真正生效的 cap → 放行
5. 都不满足 → 拒绝

---

## 4. Capability 的两阶段：grant 与 attach

这是身份相关踩坑的核心，务必理解清楚：

- **grant**（`engine.rs::capability_grant` 或 `object_birth` 内联逻辑）
  只是往 `ctx.pending_capabilities` 里 push 一条记录，声明"某个
  capability_id 授予了某个 grantee"。这条记录**要到 commit 时才
  真正写进全局 capability graph**。
- **attach**（`engine.rs::attach_capability`）是把某个 capability_id
  push 进 `ctx.capabilities`，代表"本事务当前执行上下文已经持有
  并且可以立即使用这张 cap"，不需要等 commit。

**grant 不等于 attach。** 只 grant 不 attach，意味着这张 cap
存在但当前事务里没人能用它（除非 grantee 恰好等于
current_object/capability_context）。

`OBJECT_BIRTH` 内部会自动 grant 新对象的 self-AdminCap
（grantee == resource == 新对象自己），并在 Machine 路径上
显式 attach 到本事务 `ctx`，使同一事务内 CALL 能立即通过审计。
OBJECT_BIRTH **不**切换身份。

---

## 5. 当前状态快照（2026-08-14）

### 已完成、已测试锁定：

| 项目 | 状态 | 回归测试 |
|---|---|---|
| ObjectLink 不再隐式切身份 | 完成 | machine_object_link_security.rs |
| Machine dispatch 无 `_ => {}` 死代码 | 完成 | 编译期保证（non-exhaustive match） |
| OBJECT_BIRTH 不再隐式切身份 | 完成 | object_birth_self_call.rs |
| OBJECT_BIRTH 自动 attach self-AdminCap | 完成 | object_birth_self_call.rs |
| CALL 为执行期唯一完整 identity switch | 完成 | call_access_intent.rs 等 |
| WorldService 跨对象 authorize 再 enter | 完成 | security / multi_object 相关测试 |
| Session / Host Bootstrap 文档化 | 完成 | 见第 7 节；审计 docs/IDENTITY_DRIFT_AUDIT_20260814.md |

### 已知边界（非 bug）：

- Host `tx_create_object` 在 `current_object == 0` 时有 **bootstrap exception**
  （见第 7 节），与 Machine OBJECT_BIRTH 不切换身份 **不冲突**（作用域不同）。
- `enter_object` 是底层原语，不是执行期 identity switch。
- `begin_in_object` 是 session/transaction bootstrap，不是 CALL。
- `Machine::set_execution_object` 仅测试/引导。

---

## 6. 改动这个子系统时的检查清单

1. 改 `Instruction` 枚举字段类型前，先问：这个值可能来自
   运行时（前序指令产生）吗？是 → 必须是 `Operand`，不能是裸值。
2. 改完 `instruction.rs`，必须同步改 `assembler.rs`（解析）、
   `instruction_codec.rs`（encode + decode 两处）、`machine.rs`
   （dispatch，如果字段类型变了通常需要 `resolve_operand`）。
3. 任何涉及 `current_object` 赋值的改动，先搜索
   `grep -n "current_object" -r src/`，确认改动前后这个值的
   语义没有被破坏（谁读它、读出来干什么）。
4. 任何"这个 case 反正安全可以跳过审计"的想法，先写测试验证
   走正规路径（CALL + authorize_intent）是否真的可行，不要
   凭直觉判断。
5. 改完必须 `cargo build` 全部干净，然后 `cargo test` 确认全绿。
6. 涉及安全/权限的改动，必须补一个"恶意路径必须拒绝 +
   合法路径必须成功"的对照测试，不能只测其中一边。
7. **禁止**新增未经 `authorize_intent` 的生产执行期 identity switch；
   Host 跨对象路径必须先 authorize 再 `enter_object`。
8. 不要把 Session Bootstrap 误改成 CALL，也不要把 CALL 误改成 bootstrap。

---

## 7. Session / Host Bootstrap

本章正式区分 **执行期 Identity Switch** 与 **Session / Host Bootstrap**。
依据：`docs/IDENTITY_DRIFT_AUDIT_20260814.md` 五项决策。

### 7.1 执行期 Identity Switch（Machine / VASM）

```text
CALL
  → authorize_intent(AccessIntent::Call)
  → 保存 CallFrame（parent_object, caller_capability_context, registers, return_pc）
  → current_object = target
  → capability_context = target

RETURN
  → 从 CallFrame 恢复 current_object + capability_context + registers + pc
```

**结论：CALL 是 Machine 执行期唯一正式的完整 identity switch。**

- 必须先 `authorize_intent`。
- 同步切换 `current_object` 与 `capability_context`。
- 可由 RETURN 从 CallFrame 恢复。
- OBJECT_BIRTH / OBJECT_LINK **不**构成执行期 identity switch。

### 7.2 Session Bootstrap（WorldService / Host）

```text
tx_begin(actor)
  → 创建新的 TransactionContext（begin 或 begin_in_object）
  → actor > 0 时：current_object = actor；capability_context = actor
  → SessionState 持有该 TransactionContext
```

**Session Bootstrap 是「创建一个新的执行上下文」，不是在已有执行上下文中从 A 偷换成 B。**

- 发生在事务/会话**建立时**，不在 Machine instruction dispatch 中。
- 不经过 `authorize_intent`，也不维护 CallFrame。
- 与 CALL **不同构**，不得解释为执行期 identity switch。

### 7.3 Host Object Birth Bootstrap

当 `tx_create_object` 发生时：

```text
if current_object == 0 {
    // Host Session Bootstrap exception
    current_object = new_object
    capability_context = new_object
}
```

明确：

- **只**发生在 session 尚无 acting object（`current_object == 0`）时。
- 已经存在 actor 时**不会**自动切换到 child。
- **不是** CALL；没有 CallFrame。
- **不构成**执行期 identity switch。
- **不绕过** authorize_intent（目标 id 由本 session 刚 birth，且仅 bootstrap 无 actor 的会话）。
- 与 Machine `OBJECT_BIRTH`「不自动切换身份」**不冲突**：Machine 路径靠后续 CALL 进入；Host 路径无指令流/Call 栈，在无 actor 的 session 上把首个 birth 对象设为 working object 是 session 建立期例外。

正式决策：**DOCUMENT AS BOOTSTRAP EXCEPTION**（保留行为，写清边界）。

### 7.4 `enter_object`

`TransactionContext::enter_object` 是**底层寻址原语**：

- 只改变 `current_object`。
- **不**修改 `capability_context`。
- **不等价于** Machine CALL。
- **不是**完整 identity switch。

规则：

- 调用者负责保证授权（生产跨对象路径必须先 `authorize_intent` 成功）。
- **禁止**将其用于新的执行期身份切换路径。
- Host 跨对象路径只有在 `authorize_intent` 成功之后才能使用它。

正式决策：**KEEP INTERNAL**。

### 7.5 `begin_in_object`

正式定义：

> **`begin_in_object` = Session / Transaction Bootstrap API**

- 创建 `TransactionContext` 并初始化 `current_object`。
- **不**代表执行期 CALL。
- **不**建立 CallFrame。
- `capability_context` 是否初始化由上层 session API 决定（如 `tx_begin` 在 actor > 0 时自行设置）。

正式决策：**OFFICIAL SESSION BOOTSTRAP**。

### 7.6 `Machine::set_execution_object`

- 同时设置 `current_object` 与 `capability_context`。
- **仅**用于测试 / 引导，不是生产身份切换 API。
- 正式决策：**KEEP TEST/BOOTSTRAP ONLY**。

### 7.7 总模型（一张图）

```text
执行期（Machine / VASM）:

    CALL
      → authorize_intent(AccessIntent::Call)
      → current_object + capability_context
      → CallFrame
      → RETURN 恢复

Session 建立（WorldService / Host）:

    tx_begin(actor)
      → TransactionContext（begin / begin_in_object）
      → SessionState

Host 首对象 Bootstrap:

    tx_create_object
      → OBJECT_BIRTH
      → 如果 current_object == 0
      → current_object = capability_context = new_object

Host 跨对象:

    authorize_intent(Call(target))
      → enter_object(target)   // 仅改变 current_object
      → capability_context 保持 session actor
      → 业务操作

Object Birth（Machine）:

    OBJECT_BIRTH
      → 创建 + grant/attach self-AdminCap
      → 不自动 CALL / 不切换身份
```

### 7.8 安全结论（冻结）

- 未发现未经 `authorize_intent` 的生产执行期身份绕过。
- WorldService 跨对象路径均在 `enter_object` **之前** `authorize_intent`。
- `tx_create_object(current_object==0)` 是 session bootstrap 例外，不是执行期绕过。
- CALL 仍是唯一执行期完整 identity switch。

---

## 8. 历史要点（Identity Switch 死结）

2026-08-10：OBJECT_BIRTH 曾两次出现「隐式 enter 是否安全」的反复。最终：

- **不**恢复 OBJECT_BIRTH 的 `enter_object(id)`。
- 改为 birth 后 attach self-AdminCap，使 CALL 真正可审计通过。
- 回归：`tests/object_birth_self_call.rs` + `tests/machine_object_link_security.rs`。

教训：没有实测验证替代路径是否可行时，不要恢复「看起来安全的隐式例外」。

2026-08-14：P0 Identity Drift 审计确认 Host bootstrap 与执行期模型作用域不同，定性为文档边界问题而非 Kernel 安全 bug；本文件第 7 节为正式收口。
