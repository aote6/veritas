veritas架构说明


---

## 身份切换死结的最终解法 (2026-08-10 三度修正)

### 事情经过（如实记录，包括反复）

1. 最初审计发现 `ObjectLink` 用 `enter_object(from)` 自我授权绕过 capability
   graph，判定为安全漏洞，删除修复（见上文"P0 安全修复"节）。
2. 同一逻辑推广到 `OBJECT_BIRTH`：删除其 `enter_object(id)` 自动切身份，
   改为身份切换只能经过 CALL。删除后 `world_demo.vasm` 测出：本轮虽然
   `cargo test` 全绿，但没人验证过"root 用 CALL 显式进入自己刚创建的
   对象"这个理应最基本的场景是否真的走得通。
3. 后续窗口（未验证 CALL 路线是否可行）判定 CALL 路线"卡死"，恢复了
   `enter_object(id)`，并论证"这个 case 安全，因为 id 是内核刚分配的
   全新对象，反正 authorize_intent 一定会通过，与 ObjectLink 那个真实
   漏洞不同构"。
4. 逐行验证这个论证：`object_birth` 把新对象的 self-AdminCap push 进
   `ctx.pending_capabilities`，但从未 `attach_capability` 到
   `ctx.capabilities`。`authorize_intent` 的 `has_pending` 分支要求
   `ctx.capabilities.contains(&g.capability_id)` 才算数（第三个 or 条件）
   或 `grantee == ctx.current_object/capability_context`（新对象自己的
   grantee 是它本身，不是 root）。也就是说：**"审计一定会通过"这个
   前提是错的——CALL 当时根本走不通，跟直接绕过一样，只是没人跑过
   这条路径**。用隐式切身份掩盖了一个从未被真正建立的授权关系，这正是
   与 ObjectLink 同构的问题，不是"不同构"。

### 最终结论（已实现，已测试锁定）

**不恢复 `enter_object(id)`。OBJECT_BIRTH 依旧不切换身份。**
改为：`OBJECT_BIRTH` 执行后，从 `ctx.pending_capabilities` 里找出刚创建的
`grantee == resource == id && cap_type == "AdminCap"` 的那条 grant，
把它的 `capability_id` 显式 `attach_capability` 到本事务 `ctx`。

效果：
- 身份切换的合法入口维持唯一——只有 CALL（先 `authorize_intent`
  审计，通过才切）。没有"这个case反正安全"的隐式例外口子。
- CALL 现在真的能通过审计走进新对象，因为 self-AdminCap 已经
  attach 到 ctx，`has_pending` 分支能查到。不是"反正会通过"，
  是"真的被检查过、真的通过"。
- 覆盖了最坏情况：root(current_object==0) 创建对象时，
  object_birth 不会给 root 发额外 grant（这条分支专门排除了
  current_object==0），过去被认为是"死结"——但死结的本质不是
  "root 没资格"，是"资格发了但没接上"，接上就好，不需要造
  process/bootstrap 那层。

### 验证

新增 `tests/object_birth_self_call.rs`：
`root_can_call_into_object_it_just_birthed` —— root 身份 birth
一个新对象后，用 CALL 显式进入它必须成功，且 current_object
必须真的切换过去。这是本轮争论最终要证明的行为，不再只靠论证
或手工跑 world_demo.vasm 确认。

`tests/machine_object_link_security.rs`（P0）和这个新测试一起，
构成了"身份切换只能经过唯一受控入口"这条不变量的完整回归覆盖。

全量测试：211 + 1 = 212 passed, 0 failed。

### 给未来的教训

这次反复本身就是一个案例：**一个"看起来安全的隐式例外"，
如果没有实测验证替代路径是否可行，很容易被当作"唯一解"而恢复
回去**。以后遇到类似"这个特权豁免反正安全"的论证，应该先问
"如果坚持走正规路径，会不会真的卡住"——如果卡住，先查卡在哪，
而不是退回隐式例外。详见新建的 `docs/IDENTITY_MODEL.md`。
EOF
```

**2. 新建 docs/IDENTITY_MODEL.md（完整系统说明，专门给新窗口用，不用每次重查）**

```bash
cat > docs/IDENTITY_MODEL.md << 'EOF'
# Veritas 执行身份与授权模型说明

最后更新: 2026-08-10

本文档目的：让任何新开的会话（人类或 AI）不需要重新翻整个仓库，
就能知道"身份切换"这个子系统现在是什么状态、为什么是这个状态、
改动一处会牵动哪里。这是之前几轮反复踩坑（同一个错误被"发现→修复
→用错误论证撤销→再次发现"）之后的沉淀，请先读完这份文档再改动
任何跟 `current_object` / capability / enter_object / CALL 相关的代码。

---

## 1. 核心不变量（Execution Identity Invariant）

> **`current_object` 的改变只能发生在经过定义和授权的转换点。**
> **普通业务操作不得隐式取得其他 Object 的执行身份。**

当前系统里，`current_object` 唯一合法的切换入口是 **CALL**
（`Instruction::Call`，见 `machine.rs` 的 dispatch）。
切换前必须先通过：

```rust
authorize_intent(&ctx, &AccessIntent::Call(object_id))
```

审计通过才允许 `ctx.current_object = object_id`。`RETURN` 从调用栈
弹出 `parent_object` 恢复身份，不需要额外审计（因为切回去的身份
本来就是切换前已经拥有、被记录在 `CallFrame` 里的身份）。

**没有其他例外。** 这条规则被违反过两次，两次都以为自己找到了
"安全的特殊情况"，两次都是错的：

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
    每个 Instruction 变体都被处理，这是刻意设计，见下方"P3"）
  ↓ kernel.rs — Kernel::handle()
    KernelCall 的统一入口，转发到 engine.rs 对应方法
  ↓ engine.rs — VeritasEngine
    真正的业务逻辑：object_birth / object_link / capability_grant /
    authorize_intent / commit 等。这是唯一的事实来源(source of truth)。
  ↓ TransactionContext (types.rs)
    单次事务的可变状态：current_object, capabilities,
    pending_capabilities, pending_links, pending_objects 等。
    commit 时把 pending_* 转换成 WalEntry 写入 WAL。

另一条并行执行路径：executor.rs (Executor)
  与 Machine 功能有重叠，处理同样的 Instruction 枚举，但状态：
  - 无寄存器，Register operand 直接报错（resolve_immediate 拒绝）
  - 不处理 Call/Return/Trap/HostCall（Machine 独占本地指令）
  这条路径目前用途不明确，STATUS.md 已记录为"待合并或废弃"的架构债。
  改 Instruction 定义时，如果 Executor 也 match 了这个变体，
  必须同步改（编译器会报错提醒，不会漏）。
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

`authorize_intent` 完整判断逻辑（engine.rs 第317行附近）：
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

这是本轮踩坑的核心，务必理解清楚：

- **grant**（`engine.rs::capability_grant` 或 `object_birth` 内联逻辑）
  只是往 `ctx.pending_capabilities` 里 push 一条记录，声明"某个
  capability_id 授予了某个 grantee"。这条记录**要到 commit 时才
  真正写进全局 capability graph**。
- **attach**（`engine.rs::attach_capability`）是把某个 capability_id
  push 进 `ctx.capabilities`，代表"本事务当前执行上下文已经持有
  并且可以立即使用这张 cap"，不需要等 commit。

**grant 不等于 attach。** 只 grant 不 attach，意味着这张 cap
存在但当前事务里没人能用它（除非 grantee 恰好等于
current_object/capability_context）。`CAPABILITY_GRANT` 指令
（`executor.rs`）自己就是 grant 完立刻 attach 的正确示范：

```rust
let cap_id = self.engine.capability_grant(ctx, ...)?;
self.engine.attach_capability(ctx, cap_id);
```

`OBJECT_BIRTH` 内部会自动 grant 新对象的 self-AdminCap
（grantee == resource == 新对象自己），但只 grant 没 attach。
2026-08-10 的最终修复就是在 `machine.rs` 的 `ObjectBirth` 分支里
补上这个 attach 步骤，让 CALL 能够在同一事务内立即使用这张 cap。

---

## 5. 当前状态快照（2026-08-10 收尾时）

### 已完成、已测试锁定：

| 项目 | 状态 | 回归测试 |
|---|---|---|
| ObjectLink 不再隐式切身份 | 完成 | machine_object_link_security.rs |
| Machine dispatch 无 `_ => {}` 死代码 | 完成 | 编译期保证（non-exhaustive match） |
| OBJECT_BIRTH 不再隐式切身份 | 完成 | object_birth_self_call.rs |
| OBJECT_BIRTH 自动 attach self-AdminCap | 完成 | object_birth_self_call.rs |
| CapabilityGrant{holder,resource} 用 Operand | 完成 | 编译验证 + 全量测试绿 |
| Call{object_id} 用 Operand | 完成 | call_access_intent.rs + 编译验证 |

### 已知未完成：

- **`world_demo.vasm` 仍是已知失效状态**。它是在 OBJECT_BIRTH
  自动切身份的旧语义下写的，从未用 CALL 进入过它创建的对象。
  现在 CALL 路径已经真正可行（见上方 object_birth_self_call.rs），
  下一步应该用 CALL + 汇编器已有的标签机制重写这个 demo，
  实现 birth → CALL 进新对象身份完成 WRITE → RETURN → 建 Link
  的完整闭环。开始前建议先读一遍 `src/machine.rs` 里 `Call`/
  `Return` 分支，搞清楚 `CallFrame` 的寄存器保存/恢复语义
  （寄存器在 CALL 时整体 clone 保存、RETURN 时整体恢复，
  子调用内不能直接看到父调用寄存器的值，需要用内存/state 传参）。
- **`tests/machine.rs` 仍是空文件**。最初审计要求的 E2E-1~4
  （单对象闭环、动态 Operand 数据流、Birth+Write+Link 全链路、
  跨对象非法操作必须失败）还没写。
- **P2**：`veritas inspect list` 和 `veritasd` 查询结果不一致的
  问题尚未深查。已确认两者共用同一个 `Kernel::with_wal_path`
  入口，不是"两套恢复路径"的问题，需要往下追是 WAL flush 时机
  还是 `list_object_ids()` 内部读取路径的问题。
- **待确认**：`Instruction::ObjectBirth` 的 `object_id` 字段
  （类型仍是裸 `ObjectId`，不是 Operand）疑似是废弃字段——
  机器执行时实际使用 kernel 动态分配的 id（`TrapResult::ObjectId`），
  没看到哪里读取这个静态字段的值。需要确认后决定删除还是补
  Operand 化。
- **Executor vs Machine 重叠**：两套并行的指令执行路径，
  职责边界不清晰，长期应该合并或明确废弃一个。

---

## 6. 改动这个子系统时的检查清单

1. 改 `Instruction` 枚举字段类型前，先问：这个值可能来自
   运行时（前序指令产生）吗？是 → 必须是 `Operand`，不能是裸值。
2. 改完 `instruction.rs`，必须同步改 `assembler.rs`（解析）、
   `instruction_codec.rs`（encode + decode 两处）、`machine.rs`
   （dispatch，如果字段类型变了通常需要 `resolve_operand`）。
   `executor.rs` 是否需要改，编译器会告诉你（如果它 match 了
   这个变体）。
3. 任何涉及 `current_object` 赋值的改动，先搜索
   `grep -n "current_object" -r src/`，确认改动前后这个值的
   语义没有被破坏（谁读它、读出来干什么）。
4. 任何"这个 case 反正安全可以跳过审计"的想法，先写测试验证
   走正规路径（CALL + authorize_intent）是否真的可行，不要
   凭直觉判断。
5. 改完必须 `cargo build` 全部干净（含 warning 里的
   non-exhaustive match 检查），然后 `cargo test 2>&1 | grep -E
   "test result|FAILED"` 确认全绿，两步都不能跳过。
6. 涉及安全/权限的改动，必须补一个"恶意路径必须拒绝 +
   合法路径必须成功"的对照测试，不能只测其中一边。


