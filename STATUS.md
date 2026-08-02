=== Veritas Kernel v0.3 ===

日期: 2026-07-31
分支: main

## 已实现

### 内核原语
- BEGIN / COMMIT / ABORT（OCC冲突检测 + 快照隔离 + ReadFutureVersion）
- READ / WRITE（读写屏障 + 版本追踪）
- EFFECT（幂等副作用队列 + EffectAck + 崩溃后未确认重试）
- SAVEPOINT / ROLLBACK_TO（含pending_objects和pending_links回滚）
- OBJECT_BIRTH（唯一性排他 + 零痕迹ABORT + WAL持久化 + 全局注册表）
- OBJECT_LINK（自环防御 + 拓扑固化 + WAL持久化 + ABORT零痕迹）
- OBJECT_DEATH（级联能力撤销 + 拓扑清理 + 幂等重复检测）

### 运行时服务
- WAL：序列化/反序列化/崩溃恢复/Effect重试/scope变更重放
- ScopeRegistry：struct_version独立维护 + 幻读防御
- CapabilityGraph：委托/撤销/级联撤销，主权归属ObjectId
- StateStore / StateMemory：版本化状态存储，确定性哈希
- Checkpoint：state_root快照与恢复
- ExecutionHistory：ReplayRecord记录

### 指令层 / 虚拟机
- ISA：22条指令，含编解码（instruction_codec）
- ProgramImage：校验和防篡改
- Machine：boot / step / run / trap_frame
- ExecutionContext：Trace / Event / Statistics / Receipt
- ReplayVerifier：确定性重放校验
- ModuleImage / ModuleLoader：.vmod格式加载
- Assembler：汇编到指令序列

### 崩溃恢复
- WAL Replay重建state_map / scope_map / object_registry / topology
- tx_id_counter续接（不与历史事务撞车）
- Effect重试 + 去重（基于idempotency_key）

### 权限模型（P23.2，2026-07-31架构决策）
- capability机制默认关闭（TransactionContext.capability_enforced = false）
- 需要强制校验的场景显式调用 Machine::enable_capability_enforcement()
  或 TransactionContext::enforce_capability()
- 设计原因：P23.2最初实现将capability校验做成全局强制，
  破坏了全部28个不涉及capability的既有测试（recovery/savepoint/
  scope/replay等）。capability是执行域规则，不应侵入基础执行层。
  详见commit 2f3f656。

### Call/Return跨Object调用（P24，2026-08-01）
- 修复：`Instruction::Call`/`Instruction::Return`此前在`Machine::step()`
  中完全未被处理，全部落入`_ => {}`兜底，是纯粹的未实现功能（不是回归）
- 实现：`Machine`新增`call_stack: Vec<(usize, ObjectId)>`字段，
  `Call{object_id, entry_pc}`压栈保存(return_pc, saved_current_object)、
  切换current_object、跳转pc；`Return`弹栈恢复。当前设计为
  caller/callee共享同一TransactionContext，不新开事务
  （instruction.rs注释明确：暂不涉及独立代码空间）
- 测试：153 tests passing（新增test_call_switches_current_object_and_isolates_memory）

### P24 最终方案：Call/Return在同一事务内切换current_object（2026-08-01）
- Call：保存return_pc+parent_object到call_stack，切换current_object，
  不创建新事务。caller和callee共享同一TransactionContext
- Return：从call_stack恢复current_object和pc，不创建新事务
- Commit：成功后重置TransactionContext（保留current_object），
  新事务拥有新snapshot和空read_set/write_set
- 删除P24.1的"Call/Return无条件创建新tx"方案——该方案导致
  caller在Call前未提交写入被无条件丢弃，比原始bug更严重
- 153 tests passing
## 已知限制 / 技术债

- engine.rs 2998行，占全项目39%，建议后续拆分为
  engine_capability.rs / engine_state.rs / engine_objects.rs
  （拆分方案已在体检中确认，尚未执行，属于有意暂缓的重构）
- WalWriter::open 在IO错误时直接panic（.expect），未走Result向上传递，
  磁盘满/权限异常等场景会导致进程崩溃而非优雅报错
- bytes_to_u64 对输入长度做了unwrap假设，未做长度校验的防御式处理
- extension模块无任何测试覆盖
- state_memory模块的restore/snapshot路径缺少独立测试
  （现有测试仅覆盖sparse write/read + hash）

## 测试
- 152个测试全部通过，0 ignored，0 failed
- 全量测试（不加任何filter/skip）已连续验证3次，非偶然通过
- 测试隔离：每个测试实例通过thread_local + 计数器生成独立WAL路径，
  避免并发测试互相踩踏（2026-07-31修复，此前曾因WAL路径共享
  导致测试挂起，误判为死锁）

## 依赖关系
- engine.rs 依赖最广（13个内部模块），machine.rs次之（7个）
- 全部为单向依赖，无循环依赖

## 工作流提醒（写给下一次session的自己或AI）
- 任何改变commit/begin/write等核心路径默认行为的改动，
  必须在commit message里显式标注"breaking"或"影响范围"，
  不能只写"N个测试通过"这类不描述副作用的摘要
- 修改核心校验逻辑后，必须跑一次不加--skip、不加--test-threads=1的
  完整默认并行测试，确认没有引入新的竞争条件
- 每次全量测试后检查是否有未被.gitignore覆盖的临时文件产生
  （本次修复中wal_*.log曾一度险些被提交）

### P24 隐患修复：禁止嵌套 Commit（2026-08-01）
- 根因：callee commit 不是"丢弃"caller 写入，而是把 caller 未提交的
  写入**提前持久化**。caller 和 callee 共享同一 TransactionContext，
  callee commit 将整个 write_set（含 caller 部分）原子落盘，caller
  的事务被静默拆成两半
- 修复：Machine 层检查 call_stack，非空时拒绝 Commit 并 Abort。
  依据宪法 transaction.md：Transaction 不可嵌套，CALL/RETURN 不改变
  Transaction 边界，Commit 只能在最外层执行
- P24 测试已更新：callee 不再 Commit，由 caller 在最外层统一提交
- 155 tests passing

### P29：Capability 强制校验与硬件级权限拦截（2026-08-01）
- 问题：P23.2 的 capability 强制校验打开后破坏了28个不涉及 capability
  的老测试（recovery、savepoint、replay等），当时临时关闭了开关
- 方案：capability_enforced 默认 false，通过 enforce_capability()
  显式开启。Machine 层捕获 PermissionDenied 转为 AccessDenied Trap
- 同步修复：
  - purge_subtree_strictly 增加 grants.remove 物理清理
  - CapabilityGrant 执行时自动 attach cap_id 到 TransactionContext
  - test_write_without_capability_rejected 改用 Trap 断言
- 166 tests passing
- 已知技术债：capability_enforced 默认 false 是过渡状态，
  目标是默认 true，需要后续逐步改造测试使其携带合法 capability
- 教训：注释假设（"AdminCap grant should remain"）和代码实现
  （purge_subtree_strictly 无差别清空）从一开始矛盾，写测试前
  应先确认不变量有对应的实现保证

### P30 / P30.1：ObjectRecord 一等实体与行为 API（2026-08-02）
- ObjectRecord { id, object_type, state, body } 取代裸 ObjectState 注册表
- ObjectType: StateObject | ModuleObject
- ObjectBody: State | Module { code/import/export/verification_rule }
- 行为 API：is_alive() / is_frozen() / is_dead()（委托 ObjectState）
- engine/view 读路径改用行为 API；写路径与 graph 投影保持 ObjectState
- 不变量：Object 从状态标签提升为运行时一等实体，事务/WAL/Capability/Recovery 无语义漂移
- commit: b4a8a64

### P8.1：OWNS 死亡级联（2026-08-02）
- 实现：expand_owns_death_closure()，在 commit 边界展开
- 传播源：topology（已提交）+ pending_links（本事务）
- 切断条件：pending_unlinks 中的边不参与传播
- 语义：A --OWNS--> B 且 death(A) ⇒ B 进入 pending_deaths（传递闭包）
- REFERENCES 不级联（对照测试锁定）
- 不变量：Owner death implies owned object death
- Object Death 不再是单对象状态翻转，而是生命周期事件：
  terminal state + OWNS closure + capability revoke(holder) + topology retain + WAL
- 测试：p8_1_owns_cascade_single / chain / references_no_cascade
- commit: 6da24eba
- 全量：100/100 passed

## Object Death 宪法兑现进度

| 能力 | 状态 | 阶段 |
|------|------|------|
| terminal state transition (Alive/Frozen→Dead) | ✅ | O4 |
| dead handle invalidation | ✅ | O4 |
| death irreversibility | ✅ | O4 |
| OWNS cascade (transitive) | ✅ | P8.1 |
| capability revoke by holder | ✅ | P8-final |
| topology physical cleanup (retain) | ✅ | P8.2 物理层 |
| LinkType semantic cleanup (DEPENDS_ON 通知等) | ⏳ | P8.2 |
| capability revoke by resource | ⏳ | P8.3 |
| Death Event 分发器抽取 | ⏳ | P8.4 |

## P8.2 前置观察："通知"机制现状

宪法 link.md：DEPENDS_ON 的 to 死亡时，from 应收到 TRAP（通知信号）。

当前代码中已有的相关机制（尚未接到 Link 死亡路径）：
- TrapReason / TrapFrame：执行层硬件级中断（InvalidOpcode、AccessDenied 等）
- ExecutionEvent：执行轨迹记录（InstructionStart/End、Trapped、Commit/Abort）
- PendingEffect / EffectQueue：事务副作用队列（幂等、崩溃重试）
- 无独立的 Object 级 Notification / DependencyInvalidation 通道

P8.2 设计前必须先选定：DEPENDS_ON 的"通知"落在 Trap、Effect、还是新原语。
不要在未选定载体前实现假通知。
