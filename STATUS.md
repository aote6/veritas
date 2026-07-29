Veritas Kernel V0 - 项目状态

当前版本 Phase 2.4

已完成

Phase 1.0 事务内核 BEGIN READ WRITE COMMIT ABORT 完成
Phase 1.1 读自己的写 盲写保护 fail-fast快照隔离 完成
Phase 2.1 WalRecord WalWriter 文本行日志格式 完成
Phase 2.2 commit接入WAL 先写日志后改内存 完成
Phase 2.3 RecoveryManager 启动回放WAL恢复状态 完成
Phase 2.4 恢复相关单元测试 损坏日志 空日志 完成

测试覆盖

当前测试总数 17个 全部通过

Engine测试 12个

test_basic_transaction 基本事务流程 Phase 1
test_write_conflict 写冲突检测 Phase 1
test_isolation 未提交修改不可见 Phase 1
test_read_your_writes 读自己的写 Phase 1.1
test_blind_write_conflict 盲写冲突检测 Phase 1.1
test_read_future_version_aborts fail-fast快照隔离 Phase 1.1
test_wal_file_created WAL文件在Commit后存在 Phase 2.1
test_recovery_after_commit Commit后重启恢复一致 Phase 2.3
test_multiple_commits_recovery 多次提交后恢复正确 Phase 2.3
test_empty_wal_recovery 空WAL恢复 Phase 2.3
test_recovery_then_new_transaction 恢复后新事务冲突检测 Phase 2.3

WAL模块测试 5个

test_serialize_roundtrip 序列化反序列化往返
test_incomplete_line 不完整行返回None
test_wal_write_and_read 写入并读取WAL文件
test_multiple_records_recovery 多条记录恢复
test_corrupted_wal_recovery 损坏日志恢复

已修复的问题

Phase 1.1

1 读自己的写
read优先查WriteSet再查全局Store
测试 test_read_your_writes

2 盲写保护
write时隐式补录当前版本到ReadSet
确保写-写冲突被commit捕获
测试 test_blind_write_conflict

3 fail-fast快照隔离
读到超前版本立即ABORT
替代真MVCC留待后续实现
测试 test_read_future_version_aborts

4 原子版本号
fetch_add替代load加store

Phase 2

5 WAL持久化
文本行格式 hex编码二进制值
Commit顺序 冲突检测 生成WalRecord append sync 修改内存 版本号递增
测试 test_wal_file_created

6 崩溃恢复
启动时自动从wal.log恢复
测试 test_recovery_after_commit test_multiple_commits_recovery

7 损坏日志容错
忽略最后半条记录 前面的全部恢复
测试 test_corrupted_wal_recovery

8 并行测试隔离
每个测试使用独立的WAL文件路径
原子计数器保证唯一性
修复 test_wal_file_created test_recovery_after_commit 等并行冲突

已知局限 Phase 2

快照隔离
采用fail-fast OCC近似 不维护历史版本链
读到超前版本时直接ABORT
真MVCC版本链留待Phase 3

性能
WAL每条记录都fsync 未批量
冲突重试由上层负责 建议使用指数退避加随机抖动
无并发读优化

功能
Scope作用域未实现
Effect效果系统未实现
Contract契约系统未实现

后续计划

Phase 3 MVCC版本链加垃圾回收 高优先级
Phase 4 Scope作用域加防幻读 高优先级
Phase 5 Effect效果系统 中优先级
Phase 6 Contract契约系统 中优先级

项目结构

veritas_kernel
  Cargo.toml hex加tempfile依赖
  Cargo.lock
  STATUS.md 项目状态
  README.md 项目说明
  LICENSE GPL-3.0
  docs
    STATUS.md 文档索引
    Veritas_设计文档_v0.4.md
    Veritas_运行时数据模型标准.md
  src
    main.rs 主程序
    types.rs 核心类型定义
    engine.rs 事务引擎加WAL集成
    wal.rs WAL模块

最后更新 2026-07-29
