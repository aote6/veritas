Veritas Kernel — 项目状态

当前版本: V0.3 P1 (WAL 扩展 + Effect 崩溃恢复 + tx_id 续接)
配套设计文档: Veritas V0.6 / 运行时数据模型标准 V1.0

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
已完成
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

P0 — Scope 幻读防御 + Savepoint 修复
- ScopeRegistry (src/scope_registry.rs) 独立于状态存储的 Scope 管理
- ScopeEntry { members, struct_version, owner }
- ENUM_SCOPE 原语: 读取成员列表并记录 struct_version 到 ReadSet.scopes
- BIND_TO_SCOPE / UNBIND_FROM_SCOPE 写入 ctx.scope_write_set
- COMMIT 扩展冲突检测: detect_scope_conflict() → PhantomConflict
- Savepoint 支持 scope_write_set 回滚 (修复了 touch_scope_write 绕过事务的 bug)
- ReadSet 新增 scopes 字段, WriteSet 新增 scope_changes
- 测试: 幻读保护、只读 Scope 不误报、savepoint 回滚 scope 绑定

P1 — WAL 格式扩展 + Effect 崩溃恢复 + tx_id 续接
- WalRecord → WalEntry 三态枚举 (Commit / EffectAck / Checkpoint)
- Commit 记录包含: writes + scope_changes + effects
- EffectAck 记录副作用执行确认
- Checkpoint 记录预埋 (P2 实现)
- 崩溃恢复完整流程:
  · 重放 state + scope + effect 全部信息
  · RecoveryManager::apply_records 返回 (state_map, scope_map, pending_effects, max_tx_id)
  · ScopeRegistry::from_map 直接从 WAL 重建
  · 未确认副作用自动重试并补写 EffectAck
  · tx_id_counter 从 max_tx_id + 1 续接，不撞车
- commit() 改写: effects 先塞进 WAL 记录再执行，执行后逐条写 EffectAck
- 测试: scope 结构版本重启保持、Effect 崩溃重试、EffectAck 抑制重复重试、
  tx_id 续接不撞车、序列化往返 (Commit/EffectAck/Checkpoint)

阶段 1-5 (P0 前已完成)
- 事务内核: BEGIN / READ / WRITE / COMMIT / ABORT
- 快照隔离: fail-fast OCC, ReadFutureVersion 检测
- 读自己的写: read() 优先查 WriteSet
- 盲写保护: write() 隐式补录版本到 ReadSet
- WAL: 文本行格式, 先写日志后改内存, fsync
- 崩溃恢复: 启动时回放 WAL, 损坏日志容错
- 并发压力测试: 12 线程 × 50 操作 = 600 次提交
- Scope 幻读保护: 幽灵状态 + 结构版本计数器
- Effect 原语: 暂存副作用, 幂等键 {tx_id}-{seq}, 临界区外执行
- Savepoint / ROLLBACK_TO

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
测试覆盖
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

当前测试总数: 36 个, 全部通过

Engine 基本测试 (12)
  test_basic_transaction              基本事务流程
  test_write_conflict                 写冲突检测
  test_isolation                      未提交修改不可见
  test_read_your_writes               读自己的写
  test_blind_write_conflict           盲写冲突检测
  test_read_future_version_aborts     fail-fast 快照隔离
  test_wal_file_created               WAL 文件在 Commit 后存在
  test_recovery_after_commit          Commit 后重启恢复一致
  test_multiple_commits_recovery      多次提交后恢复正确
  test_empty_wal_recovery             空 WAL 恢复
  test_recovery_then_new_transaction  恢复后新事务冲突检测
  test_concurrent_transactions        并发压力测试 (12线程×50操作)

Scope 测试 (4)
  test_family_shared_limit_phantom_read_protection  幻读保护 (设计文档 5.1 旗舰示例)
  test_scope_read_only_does_not_cause_false_conflict 只读 Scope 无假冲突
  test_scope_bind_rolls_back_with_savepoint          savepoint 回滚 scope 绑定
  test_scope_struct_version_survives_restart         Scope 结构版本经 WAL 持久化

Effect 测试 (2)
  test_effect_not_executed_on_abort     ABORT 时副作用不执行
  test_effect_executed_after_commit     COMMIT 后副作用执行 + EffectAck

崩溃恢复集成测试 (2)
  test_crash_recovery_retries_unacked_effect  未确认副作用重启重试 + 补写 EffectAck
  test_tx_id_counter_survives_restart          tx_id 重启续接不撞车

Savepoint 测试 (5)
  test_savepoint_basic                 基本保存点
  test_savepoint_nested                嵌套保存点
  test_savepoint_effect_rollback       副作用随保存点回滚
  test_savepoint_not_found             不存在的保存点
  test_savepoint_multiple_states       多状态保存点

WAL 模块测试 (11)
  test_commit_serialize_roundtrip              Commit 三合一序列化往返
  test_effect_ack_roundtrip                    EffectAck 序列化往返
  test_checkpoint_roundtrip                    Checkpoint 序列化往返
  test_incomplete_line                         不完整行返回 None
  test_wal_write_and_read                      写入并读取 WAL
  test_multiple_records_recovery               多条记录恢复
  test_empty_wal_recovery                      空 WAL 恢复
  test_corrupted_wal_recovery                  损坏日志容错
  test_effect_retry_after_crash_without_ack    Commit 无 Ack → 标记待重试
  test_effect_not_retried_if_acked             Commit+Ack → 不重试
  test_scope_changes_replayed_into_scope_map   Scope 变更经 WAL 重建

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
当前代码结构
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

src/
  main.rs             主入口
  types.rs            核心类型定义 (StateEntry, ScopeEntry, ReadSet, WriteSet, TransactionContext, etc.)
  engine.rs           事务引擎核心 (~900行, 含全部测试)
  store.rs            状态存储 (StateStore)
  wal.rs              WAL 模块 (WalEntry 三态枚举, RecoveryManager)
  scope.rs            Scope 扩展 trait (ScopeExt)
  scope_registry.rs   Scope 注册表 (独立于状态存储的结构版本管理)
  effect.rs           副作用队列 (EffectQueue)
  extension.rs        扩展点接口 (Extension trait)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
已知局限
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

- 快照隔离: fail-fast OCC 近似, 不维护历史版本链
- WAL: 文本格式 (非二进制 CRC32), 每条记录 fsync, 未批量
- 冲突重试: 由上层负责, 建议指数退避加随机抖动
- enumerate_scope: 未做 ReadFutureVersion 类比检测 (struct_version 与 global_version 未对齐)
- Checkpoint: 原语已定义, 但截断逻辑未实现 (P2)
- 能力系统 (Capability): GRANT/REVOKE/DELEGATE/HOLD_CHECK 未实现
- 契约系统 (REQUIRE/ENSURE/INVARIANT) 未实现
- 模块系统 (LOAD/LINK/UNLOAD) 未实现
- 错误脱敏: 未实现
- 单机假设: 分布式事务、网络分区、共识协议不在范围内
- Effect: "至少一次"语义, 不保证恰好一次

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
后续计划
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

P1.5  WAL CRC32 校验 + 二进制格式 (可选)
P2    CapabilityGraph / 能力系统 (GRANT, REVOKE, DELEGATE, HOLD_CHECK)
P3    契约系统 (REQUIRE, ENSURE, INVARIANT)
P4    模块系统 (LOAD, LINK, UNLOAD, CAP_REQUIRE, REENTRANT_ALLOW)
P5    Checkpoint 截断 + WAL 垃圾回收
P6    MVCC 版本链 + 垃圾回收
P7    错误脱敏
P8    分布式扩展 (多机共识)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
设计文档偏差 (明确记录)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. WAL 格式: 标准要求二进制+CRC32, 当前使用文本格式。
   理由: Termux 环境下 cat wal.log 直接可读, 排障效率优先。
   CRC32 留作独立 P1.5。

2. WalEntry::Commit.effects 字段: 标准原定义未包含此字段,
   但设计文档 4.8 节崩溃恢复流程要求扫描 Commit 中的副作用。
   此处补充 effects 字段以消解文档间矛盾。

3. StateId 哈希算法: 标准要求 SipHash/xxHash64 固定种子,
   当前使用 FNV-1a。原型阶段 FNV 足够, 后续可切换。

4. StateEntry 缺少 owner 和 path 字段: 标准要求记录所属模块和完整路径,
   模块系统未实现前暂不添加。

5. 全局状态存储用 Mutex 而非 RwLock: 与标准偏差, 原型阶段简化。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
设计原则执行情况
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

原则一: 保证优先于性能 ✅
  - 每条 WAL 记录 fsync, 不批量
  - 提交临界区串行化, 消除 TOCTOU
  - Effect 先写 WAL 后执行, 崩溃可重试

原则二: 原语最小化 ✅
  - ENUM_SCOPE 显式原语, 不隐式推断
  - BIND_TO_SCOPE / UNBIND_FROM_SCOPE 强制指定成员

原则三: 不可绕过优先于便利性 ✅
  - 状态修改必须在事务内
  - Scope 结构变更写入 ctx.scope_write_set, commit 前可回滚
  - 无后门修改 ScopeRegistry

最后更新: 2026-07-29
