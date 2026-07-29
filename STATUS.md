Veritas Kernel V0 - 项目状态

当前版本 Phase 5 (Effect 骨架完成)

已完成

Phase 1.0 事务内核 BEGIN READ WRITE COMMIT ABORT 完成
Phase 1.1 读自己的写 盲写保护 fail-fast快照隔离 完成
Phase 2.1 WalRecord WalWriter 文本行日志格式 完成
Phase 2.2 commit接入WAL 先写日志后改内存 完成
Phase 2.3 RecoveryManager 启动回放WAL恢复状态 完成
Phase 2.4 恢复相关单元测试 损坏日志 空日志 完成
Phase 2.5 并发压力测试 12线程600次提交 完成
Phase 4 Scope 幽灵状态 结构版本计数器 幻读保护 反向回归 完成
Phase 5 Effect 骨架 EFFECT原语 幂等键 临界区外执行 完成

测试覆盖

当前测试总数 22个 全部通过

Engine测试
test_basic_transaction 基本事务流程
test_write_conflict 写冲突检测
test_isolation 未提交修改不可见
test_read_your_writes 读自己的写
test_blind_write_conflict 盲写冲突检测
test_read_future_version_aborts fail-fast快照隔离
test_wal_file_created WAL文件在Commit后存在
test_recovery_after_commit Commit后重启恢复一致
test_multiple_commits_recovery 多次提交后恢复正确
test_empty_wal_recovery 空WAL恢复
test_recovery_then_new_transaction 恢复后新事务冲突检测
test_concurrent_transactions 并发压力测试 12线程600次提交

Scope测试
test_family_shared_limit_phantom_read_protection 幻读保护
test_scope_read_only_does_not_cause_false_conflict 只读Scope无假冲突

Effect测试
test_effect_not_executed_on_abort ABORT时副作用不执行
test_effect_executed_after_commit COMMIT后副作用执行

WAL模块测试
test_serialize_roundtrip 序列化反序列化往返
test_incomplete_line 不完整行返回None
test_wal_write_and_read 写入并读取WAL文件
test_multiple_records_recovery 多条记录恢复
test_empty_wal_recovery 空WAL恢复
test_corrupted_wal_recovery 损坏日志恢复

已修复的问题

Phase 1.1
1 读自己的写 read优先查WriteSet
2 盲写保护 write时隐式补录版本到ReadSet
3 fail-fast快照隔离 读到超前版本立即ABORT
4 原子版本号 fetch_add替代load+store

Phase 2
5 WAL持久化 文本行格式 hex编码 先写日志后改内存
6 崩溃恢复 启动时自动从wal.log恢复
7 损坏日志容错 忽略最后半条记录
8 并行测试隔离 每个测试独立WAL路径

Phase 4
9 Scope幻读保护 幽灵状态 结构版本计数器 复用现有冲突检测
10 Scope反向回归 只读Scope不造成假冲突

Phase 5
11 Effect原语 暂存副作用到事务队列 不立即执行
12 幂等键自动生成 {事务ID}-{序列号}
13 临界区外执行 COMMIT成功后执行副作用

当前代码结构

src/
  engine.rs  核心引擎 含WAL Scope Effect集成
  wal.rs     WAL模块
  scope.rs   Scope扩展
  types.rs   类型定义
  main.rs    主程序

engine.rs 已超过600行 准备在下一次Phase 6重构中拆分

已知局限

快照隔离 采用fail-fast OCC近似 不维护历史版本链
WAL 每条记录都fsync 未批量
冲突重试 由上层负责 建议指数退避加随机抖动
Effect ACK 尚未写入WAL 恢复时重试逻辑待实现
MVCC 版本链 留待后续性能优化阶段

后续计划

Phase 6 工程化重构 拆分engine.rs到独立模块
Phase 7 Effect ACK WAL扩展 + 恢复重试逻辑
Phase 8 MVCC 版本链 + 垃圾回收

最后更新 2026-07-29
