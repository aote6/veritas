# Veritas

[English](#english) | [中文](#chinese)

---

## English

Veritas - An abstract machine design and prototype implementation driven by the "Devil's Programmer Testing" methodology.

**Current Phase**: Phase 1.1, transaction kernel prototype (BEGIN/READ/WRITE/COMMIT/ABORT + Blind Write Protection + fail-fast Snapshot Isolation). WAL/Scope/Effect/Contract not yet implemented. Design is still evolving.

## Documentation

Design documents are located in the `docs/` directory:
- Veritas_设计文档_v0.4.md
- Veritas_运行时数据模型标准.md

## Status

Currently 6/6 unit tests passing. See [STATUS.md](STATUS.md) for details.

## License

GPL-3.0

---

## 中文

Veritas - 以"魔鬼程序员测试"为方法论的抽象机器设计与原型实现。

**当前阶段**：Phase 1.1，事务内核原型（BEGIN/READ/WRITE/COMMIT/ABORT + 盲写保护 + fail-fast 快照隔离）。尚未包含 WAL/Scope/Effect/Contract，设计仍在演进中。

## 文档

设计文档位于 `docs/` 目录：
- Veritas_设计文档_v0.4.md
- Veritas_运行时数据模型标准.md

## 状态

当前 6/6 单元测试通过。详见 [STATUS.md](STATUS.md)。

## 许可证

GPL-3.0
