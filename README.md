# Veritas

一个探索性的、以"魔鬼程序员测试"为方法论的抽象机器设计与原型实现。

**当前阶段**：Phase 1.1，事务内核原型（BEGIN/READ/WRITE/COMMIT/ABORT + 盲写保护 + fail-fast 快照隔离）。尚未包含 WAL/Scope/Effect/Contract，设计仍在演进中。

## 文档

设计文档位于 `docs/` 目录：
- Veritas_设计文档_v0.4.md
- Veritas_运行时数据模型标准.md

## 状态

当前 6/6 单元测试通过。详见 [STATUS.md](STATUS.md)。

## License

GPL-3.0
