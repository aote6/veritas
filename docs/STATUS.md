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
