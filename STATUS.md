Veritas Kernel V0 - 项目状态

当前版本 Phase 1.1

已完成
Phase 1.0 事务内核 BEGIN READ WRITE COMMIT ABORT 完成
Phase 1.1 读自己的写 盲写保护 fail-fast快照隔离 完成

已修复的问题

1 读自己的写 Read Your Writes
   read 优先查 WriteSet 再查全局 Store
   测试 test_read_your_writes

2 盲写保护 Blind Write Protection
   write 时隐式补录当前版本到 ReadSet
   确保写-写冲突被 commit 捕获
   测试 test_blind_write_conflict

3 fail-fast 快照隔离
   读到超前版本立即 ABORT
   替代真 MVCC 留待后续实现
   测试 test_read_future_version_aborts

4 原子版本号
   fetch_add 替代 load + store
   更安全更高效

测试覆盖
6个测试全部通过
test_basic_transaction ok
test_blind_write_conflict ok
test_read_future_version_aborts ok
test_write_conflict ok
test_read_your_writes ok
test_isolation ok

已知局限 Phase 1
快照隔离采用 fail-fast OCC 近似 不维护历史版本链
读到超前版本时直接 ABORT 而非返回 BEGIN 时的快照值
真 MVCC 版本链留待 Phase 3 性能优化阶段实现
冲突 ABORT 后由上层负责重试 建议使用指数退避加随机抖动
持久化 纯内存存储 无持久化 WAL 预写日志在 Phase 2 实现

后续计划
Phase 2 WAL 预写日志 加 崩溃恢复 高优先级
Phase 3 MVCC 版本链 加 垃圾回收 中优先级
Phase 4 Scope 作用域 加 防幻读 高优先级
Phase 5 Effect 效果系统 中优先级
Phase 6 Contract 契约系统 中优先级

项目结构
veritas_kernel
  Cargo.toml 项目配置
  STATUS.md 项目状态
  docs 设计文档
    Veritas 抽象机器设计规.txt
    Veritas 运行时数据模型标准.txt
  src
    main.rs 主程序演示
    types.rs 核心类型定义
    engine.rs 事务引擎加6个单元测试

最后更新 2026-07-29
