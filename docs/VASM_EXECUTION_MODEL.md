# Veritas VASM 执行模型与操作手册

> **2026-08-20 更新**：Kernel service 的 Machine 入口已统一为 `TRAP <service_id>`（0–13）。
> 旧式 `OBJECT_BIRTH` / `COMMIT` 等 **Instruction / Assembler mnemonic 已删除**。
> 下文若仍出现 `OBJECT_BIRTH` 字样，指 **Kernel 语义名 / 历史排查笔记**，汇编请写 `TRAP 0`。
> 权威 ABI：`docs/TRAP_ABI_FREEZE.md`、指令表：`docs/Veritas_指令集.md`。

写给下一个接手的 AI 或人类。目的：把执行链路与身份/授权结论讲清楚，避免重新排查一遍。

---

## 1. 整体执行链路

一个 .vasm 源文件，从文本到持久化世界状态，要经过这几层：

  .vasm 源码
    -> assembler.rs 的 assemble() / assemble_module()   把文本解析成 Vec<Instruction>
    -> instruction_codec.rs                              把 Instruction 编码成字节（.vmod 格式）
    -> ModuleLoader（module.rs）                          把字节解码回 ModuleImage，安装进 loader
    -> Machine::boot()                                    把 ProgramImage 写入 Machine 的 ram
    -> Machine::step() 循环                                逐条取指、译码、执行
    -> Instruction::Trap → KernelCall::decode_with_memory → Kernel::handle()   Kernel service（TRAP 0–13）
    -> Engine（engine.rs）                                  真正的状态存储：object_registry / topology / capability_graph / state_store
    -> WAL（wal.rs）                                        commit 时把 TransactionDelta 写入 WAL，保证持久化
    -> Recovery / Replay                                    重启或 inspect 命令时，从 WAL 重放出世界状态

写代码或排查问题时，要先想清楚自己动的是哪一层，不要在错误的层次找答案。比如"为什么 WRITE 的内容没生效"，八成不是 assembler 或 machine 的问题，是 commit 有没有真的跑到、Address 组装对不对。

---

## 2. 两条执行路径，不要混用

代码库里同时存在两套能"跑起来"的东西：

第一套：Machine（src/machine.rs）
  有寄存器文件、有调用栈（CallFrame）、有 PC、支持 CALL/RETURN 身份切换。
  这是正式路径，CLI 的 veritas run、cargo test 里写的 E2E 测试，全部走这条。

第二套：Executor（已删除，2026-08-11）
  独立实现，没有寄存器文件，遇到 Operand::Register 会直接报错，拒绝执行。
  截至 2026-08-10 审计，这个模块没有任何外部调用者，只在自己文件内部被引用。
  不要用它，也不要往里加功能，它是待清理的技术债，STATUS.md 里已经记了"待合并或废弃"。

判断依据：grep -rn "Executor::" src/ tests/ 只会在 executor.rs 自己文件里出现结果。

---

## 3. 身份切换模型（这是本仓库反复出问题的核心机制）

TransactionContext（简称 ctx）上有两个关键字段：

  current_object       当前执行身份，决定 WRITE/READ 写到哪个对象的 Address
  capability_context    当前用于 capability 校验的上下文对象

初始状态：Machine 启动时 ctx.current_object = 0（root）。

只有一条指令可以合法地改变 current_object：CALL。
其余任何指令都不应该切换身份——这是 2026-08-10 那一轮反复拉锯后锁定的最终结论（详见本文件末尾"教训"一节）。

CALL 执行时（machine.rs 里 Instruction::Call 分支）做的事，按顺序：
  1. 用 AccessIntent::Call(object_id) 走 authorize_intent 审计，没权限直接 Trap，不切换任何东西
  2. 把当前的 PC、当前的 current_object、当前整个寄存器文件的拷贝、当前 capability_context，
     一起打包成一个 CallFrame，压进 call_stack
  3. current_object 和 capability_context 都改成目标 object_id
  4. PC 跳到 entry_pc

RETURN 执行时：
  1. 从 call_stack 弹出最近一个 CallFrame
  2. current_object / capability_context 恢复成 CallFrame 里存的父调用值
  3. 寄存器文件整体替换成 CallFrame 里存的那份快照，PC 恢复成 return_pc

这意味着两件事，写 vasm 程序时必须记住：

  A. 寄存器文件是"连续"的，没有被清空或隔离。CALL 之前父调用写在寄存器里的值，
     跳进子调用之后仍然读得到（因为只是换了身份和 PC，寄存器没变）。
     这就是为什么 CALL 之前可以先把 birth 出来的对象 id（存在 R0）搬到别的寄存器
     （比如 ADD Rn, R0, R0 或配合 LOAD_CONST 做加法），因为下一次 OBJECT_BIRTH 会覆盖 R0。

  B. 子调用对寄存器的任何修改，在 RETURN 之后全部作废，因为寄存器文件会被
     父调用那份快照整体覆盖回去。子调用如果想把结果"带出去"，不能指望寄存器，
     只能通过状态层面的效果（WRITE 到某个 Address，或者建立 LINK）。

---

## 4. WRITE 指令写到哪里

kernel.write() 只是透传，真正组装写入地址的逻辑在 engine.rs 的 write() 里：

  地址 = Address { object_id: ctx.current_object, state_id: <你传的 state_id 参数> }

也就是说，WRITE 指令里的 state_id 操作数，只是同一个对象内部的"槽位号"，
真正决定写到"哪个对象"的，是当前的 current_object，不是 WRITE 指令本身的任何参数。

结论：如果想把数据写进对象 A 的状态里，必须先 CALL 进 A，让 current_object 变成 A，再 WRITE。
不能指望在 root 身份下、把 A 的 id 作为 state_id 传给 WRITE——那样写的其实是
Address(root, A的id)，也就是 root 名下一个碰巧数值等于 A 的槽位，跟对象 A 毫无关系。
这正是 P31 之前那版失效 demo 犯的错误。

---

## 5. Operand：立即数还是寄存器

一部分指令的部分字段类型是 Operand 枚举，不是裸的 u64：

  Operand::Immediate(u64)   直接是这个数值
  Operand::Register(u8)     去读某个寄存器当前的值

目前用 Operand 的字段：Read.state_id / Write.state_id / ObjectDeath.object_id /
ObjectFreeze.object_id / ObjectLink.from / ObjectLink.to / ObjectUnlink.from /
ObjectUnlink.to / Call.object_id / CapabilityGrant.holder / CapabilityGrant.resource。

注意 ObjectBirth.object_id 不是 Operand，是裸 u64，而且这个字段本身是死代码——
内核会自己分配一个全新的 id，忽略你传入的这个参数，运行结果只会写回 R0。
汇编时随便填个数字占位就行，比如 OBJECT_BIRTH 0，这个 0 没有任何实际意义。

汇编文本里怎么写：直接写数字就是 Immediate，写 R0/R1/... 这种形式就是 Register，
assembler.rs 的 parse_operand 靠前缀是不是 "R" + 数字来判断，不需要额外语法标记。

---

## 6. vasm 语法速查

必须有：module <名字> 和 version <x.y.z> 两行开头。

注释：分号 ; 开头。

标签：单独一行，名字后面跟冒号，比如 loop: 或 body:，跳转/CALL 目标直接写标签名，
      assembler 会在第一遍扫描时把标签解析成对应的 PC。

常用指令写法举例（每行一条，逗号分隔参数）：

  TRAP 0                               创建对象（ObjectBirth），新 id 写入 R0，不切换身份
  LOAD_CONST R2, 0                     把立即数 0 写入寄存器 R2
  ADD R1, R0, R2                       R1 = R0 + R2
  CALL R1, body                        身份切换进 R1 里的对象 id，跳到标签 body
  WRITE R0, "some text"                往当前身份对象的 Address(current_object, R0槽位) 写入字符串
  RETURN                               弹栈返回，身份/PC/寄存器全部恢复到 CALL 之前
  ; 链接：R0=from R1=to R2=1(owns)/0(depends_on)/2(references) 后：
  TRAP 2                               ObjectLink
  TRAP 5                               Commit，pending 变更落 WAL
  HALT                                 停机

---

## 7. CLI 三段式操作流程

第一步，把 .vasm 编译成字节码模块：

  cargo run --bin veritas -- compile world_demo.vasm world_demo.vmod

第二步，运行。run 命令要求 WAL 文件必须提前存在，用 touch 建一个空文件：

  touch ./world_demo.wal
  cargo run --bin veritas -- run world_demo.vmod ./world_demo.wal

跑完会打印 finished pc=... r0=... 和 objects in world: N。

第三步，查询持久化状态（这是从 WAL 重放出来的，走的是 Recovery 路径，
跟第二步执行时那个内存里的 Engine 不是同一个实例，但内容应该一致）：

  cargo run --bin veritas -- inspect ./world_demo.wal list
  cargo run --bin veritas -- inspect ./world_demo.wal object 1

inspect list 只显示对象和它们的存活状态，不显示 link 关系。
如果需要确认 link 是否建立，目前没有对应的 CLI 子命令，
只能在 Rust 代码/测试里用 kernel.has_link(from, to) 查询。

---

## 8. 已修复：Runtime::execute 遇到 Trap 会死循环（2026-08-14 修复）

src/runtime.rs 的 Runtime::execute() 内部循环写法：

  while !machine.is_halted() {
      machine.step()?;
  }

而 Machine::is_halted() 的定义（src/machine.rs）：

  pub fn is_halted(&self) -> bool {
      matches!(self.status, MachineStatus::Halted | MachineStatus::Aborted(_))
  }

它不认 MachineStatus::Trapped。也就是说，如果程序执行中触发了 Trap
（比如权限校验失败、指令编码错误），status 变成 Trapped 之后，
is_halted() 永远返回 false，这个 while 循环永远不会退出。

影响范围：cargo run --bin veritas -- run 这条 CLI 命令，
只要跑的程序会触发 Trap，进程就会挂死，需要手动 kill。

截至本文档写成时（2026-08-11），这个问题尚未修复，只是被绕开（2026-08-14 已修复）：
tests/machine/basic.rs 里所有 E2E 测试都没有用 Runtime::execute，
而是自己写了一个带步数上限的循环（run_bounded 函数），
手动匹配 Halted / Aborted / Trapped 三种终止状态，避免测试本身卡死。

**状态更新（2026-08-14）**：已修复。`is_halted()` 已加入 `MachineStatus::Trapped(_)` 匹配。

修复思路（已实施）：把 is_halted() 的 matches! 里加上 MachineStatus::Trapped(_)，
或者给 Runtime::execute 单独加一层步数上限保护。哪种更合适取决于
"CLI 遇到 Trap 应该报错退出还是打印 trap 原因"这个产品决策，未定，留给后续。

---

## 9. current_object 身份切换的历史教训（为什么只信 CALL）

这段历史值得完整读一遍，因为同样的错误已经在这个仓库里发生过两次，
具体细节见 STATUS.md 里"身份切换死结的最终解法（三度修正）"一节，这里只提炼结论：

第一次：ObjectLink 指令为了图省事，执行时自己调用 enter_object(from)，
临时把身份切换成 from 对象，这样 commit 阶段的权限检查
"调用者是否等于 target"就会天然成立——但这不是真的有权限，
是把检查对象换成了自己，属于自我授权，是真实的越权漏洞，已删除修复。

第二次：OBJECT_BIRTH 起初也这么干，执行完自动 enter_object(新id)。
删除这个自动切换之后，有一轮误判说"这个 case 反正安全，因为对象是刚创建的、
一定会通过审计"，于是把 enter_object 恢复了回去——但从来没有人
实测验证过"删除自动切换后，CALL 路径本身是不是真的能走通"。
逐行查证后发现：object_birth 只是把新对象的 self-AdminCap 放进了
ctx.pending_capabilities，从未真正 attach 到 ctx.capabilities，
所以 authorize_intent 的审计条件其实并不成立，"反正会通过"是错的假设，
只是因为从没实测过 CALL 路径，才没暴露出来。

最终修复：不恢复隐式切换。OBJECT_BIRTH 执行后，显式把新对象的
self-AdminCap 从 pending_capabilities 转成正式 attach 到 ctx。
这样 CALL 才是真的、被审计过、被验证过地能走通，而不是靠"反正应该没问题"
这种没有实测支撑的论证。

给未来的提醒：任何"这个身份切换/权限豁免看起来是安全的特殊情况"的论证，
如果没有对应的实测（写一个测试，实际跑一遍，看它是不是真的按预期工作），
都不要采信，也不要凭直觉恢复被删除的隐式切换逻辑。
先问"如果坚持走正规路径（也就是唯一合法入口 CALL），会不会真的卡住"——
如果卡住，去查卡在哪一步，而不是绕回隐式例外。

---

## 10. world_demo.vasm 现状（2026-08-11 验证通过）

当前 world_demo.vasm 内容：

  birth 对象 A，把 id 从 R0 挪到 R1，CALL 进 A，WRITE 一条数据，RETURN 回 root
  birth 对象 B，把 id 从 R0 挪到 R2，CALL 进 B，WRITE 一条数据，RETURN 回 root
  root 身份下直接 OBJECT_LINK R1, R2, owns（这一步不需要 CALL，
  因为两次 birth 已经把 A 和 B 各自的 self-AdminCap attach 到了同一个 ctx，
  root_link_two_children.rs 测试专门验证过这一点）
  COMMIT / HALT

对应的自动化回归测试：tests/machine/basic.rs 里的 e2e_1 到 e2e_4，
其中 e2e_3 直接读取磁盘上的 world_demo.vasm 文件本身来跑，
保证测试和实际交付的 demo 文件不会脱节——改了 world_demo.vasm
不更新测试逻辑的话，e2e_3 会因为断言不匹配而失败，不会静默过时。

CLI 实跑验证记录（2026-08-11）：
  compile 成功
  run 输出：finished pc=104 r0=2，objects in world: 2
  inspect list 输出：对象 1 和对象 2 均为 Alive

---

## 11. 给下一个接手者的排查方法论

遇到"这个功能到底有没有做完/做对"这种问题时，按这个顺序来，
不要只看 STATUS.md 或 commit message 的文字描述：

  1. 先看测试文件的实际内容和实际执行结果（cargo test 的输出），
     不要只看文件是否存在、行数是否非零。空 stub 测试和真实断言看起来都是"passed"。

  2. 关键路径要看有没有被真正调用过。一个函数/文件存在不代表它在生产路径上，
     用 grep -rln "某函数名" src/ tests/ tools/ 确认调用者，
     没有调用者的东西（比如 executor.rs）不能当作已验证的事实基础。

  3. STATUS.md 是会滞后的，最新几个 commit 的内容不一定已经写进去，
     以 git log --oneline 和 git show <commit> --stat 为准，
     STATUS.md 只作为背景参考，不作为"是否完成"的判断依据。

  4. 任何"这样应该是安全/可行的"论证，如果找不到对应的实测（测试或实跑输出），
     一律当作未验证，不要采信，也不要在此基础上继续往上叠加新功能——
     本文件第 9 节的教训就是不遵守这条原则导致的两次反复。

  5. 写新测试时，尽量让测试直接依赖生产链路上真实用到的入口
     （比如 assemble_module + Machine::boot，而不是手拼字节码），
     并且如果测试对象是某个具体交付文件（比如 world_demo.vasm），
     让测试直接读那个文件跑，而不是在测试里内嵌一份"应该等价"的源码拷贝——
     否则交付文件和测试会各自漂移，谁都不再代表真实状态。
