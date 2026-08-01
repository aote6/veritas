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

### 已知未覆盖场景 / 待设计决策（P24 follow-up，未修复）
- **caller未提交写入被callee commit静默冲掉的风险**：Commit触发的重置
  逻辑是`self.ctx = self.engine.begin()`——整个替换ctx。若caller在Call
  前有未提交写入，callee内部commit时会导致caller那笔写入所在的旧ctx
  被整体丢弃，不报错、不提交，静默丢失数据。
- 需要在以下方案中决策（未实现任何一个）：
  方案A：Call前强制要求ctx已commit（简单，限制灵活性）
  方案B：write_set按object分区，callee commit只提交该object相关写入
  方案C：Call新开子事务而非共享ctx（改变现有设计初衷）
- 测试草案 test_caller_uncommitted_write_lost_when_callee_commits
  已写出但未加入正式测试套件（预期FAIL，验证问题存在），见

### P24.1 修复：CallFrame不再保存完整TransactionContext（2026-08-01）
- 问题：CallFrame保存了整个父ctx，Return时把已commit并移除的tx复活，
  导致engine.commit检查tx_id时失败（WriteConflict）
- 修复：CallFrame改为只保存return_pc+parent_object；Return调用
  engine.begin()创建新事务，current_object恢复为parent_object
- 结果：153 tests passing，旧事务不再被复活
- 语义：Call=事务切换（新tx），Return=事务切换（新tx），
  caller/callee事务生命周期独立
  .patches/p24_debug_scripts下本次session记录，下次处理前应先跑这个
  草案确认是Err还是错误值，以判断严重程度

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
