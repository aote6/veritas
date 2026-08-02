Veritas Kernel 设计文档索引

本目录包含以下设计文档

Veritas_设计文档_v0.4.md
  抽象机器的整体架构设计（v0.4 定版）

Veritas_运行时数据模型标准.md
  运行时数据模型的定义和标准

Phase 1.1 实现状态
事务内核已实现 包含 BEGIN READ WRITE COMMIT ABORT
读自己的写 已修复
盲写保护 已修复
fail-fast 快照隔离 已实现
6 个单元测试全部通过

文档最后更新 2026-07-29

Phase 1.2 Graph 子系统实现状态
- G1 Store: 边表基础索引与出入度加速
- G2 Policy: Owns/References 关系约束与 DAG 环检测
- G3 Transaction/Journal/Replay: 事务暂存隔离、WAL 事件日志落盘与确定性重放
- G4 Recovery: 引入 ReplayMode::Recovery 与 Strict 模式，支持掉电崩溃自动丢弃 EOF 未提交事务 (Crash-consistency)
- 架构修复: 解耦 Replay 模式，解决崩溃恢复末尾未提交事务校验冲突；完成全工程 Warning 零残留清理
- 测试状态: 97 个测试用例全部通过 (单元测试 64 / Graph 22 / Object 6 / Transaction 4 / Capability 1)，0 Warnings

文档最后更新 2026-08-02
