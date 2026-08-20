# Veritas Kernel 文档索引

最后整理: 2026-08-20

## 从这里开始

| 文档 | 用途 |
|------|------|
| [../STATUS.md](../STATUS.md) | 实现进度与里程碑（历史条目只追加） |
| [../ROADMAP_NEXT.md](../ROADMAP_NEXT.md) | 下一步优先事项 |
| [../README.md](../README.md) | 项目简介 |
| [VERIFICATION_MAP.md](VERIFICATION_MAP.md) | 已验证/冻结能力地图 |

## 现行 Machine / TRAP ABI（以代码与下列冻结文档为准）

| 文档 | 用途 |
|------|------|
| [Veritas_指令集.md](Veritas_指令集.md) | 汇编助记符与 TRAP service_id 表 |
| [TRAP_ABI_FREEZE.md](TRAP_ABI_FREEZE.md) | TRAP 参数块与寄存器 ABI |
| [TRAP_ABI_ERROR_CONTRACT_FREEZE.md](TRAP_ABI_ERROR_CONTRACT_FREEZE.md) | 错误码映射 |
| [TRAP_ENTRYPOINT_CLOSURE.md](TRAP_ENTRYPOINT_CLOSURE.md) | 入口收口裁定与退役记录 |
| [USAGE.md](USAGE.md) | CLI / 上手流程 |
| [VASM_EXECUTION_MODEL.md](VASM_EXECUTION_MODEL.md) | 执行链路与排障笔记 |

## 历史 / 审计（勿当现行 ABI）

| 文档 | 说明 |
|------|------|
| [TRAP_MIGRATION_AUDIT.md](TRAP_MIGRATION_AUDIT.md) | 迁移前审计快照 |
| [TRAP_ABI_ERROR_CONTRACT.md](TRAP_ABI_ERROR_CONTRACT.md) | 错误合约工作稿（以 FREEZE 为准） |
| [ARCHITECTURE_DEBT.md](ARCHITECTURE_DEBT.md) | 技术债台账 |
| [HIDDEN_ISSUES.md](HIDDEN_ISSUES.md) | 隐藏问题记录 |
| `*_AUDIT_*.md` / `CHECKPOINT_*` | 专项审计，按日期阅读 |

## 语义与模型（勿轻改）

| 文档 | 说明 |
|------|------|
| [constitution/](constitution/) | **宪法十文档** — 硬约束 |
| [Veritas_运行时数据模型标准.md](Veritas_运行时数据模型标准.md) | 数据模型标准 |
| [Veritas_Runtime_Object_规范.md](Veritas_Runtime_Object_规范.md) | Runtime Object 规范 |
| [Veritas_设计文档.md](Veritas_设计文档.md) | 设计意图 |
| [IDENTITY_MODEL.md](IDENTITY_MODEL.md) | 身份模型 |
| [TEST_ARCHITECTURE.md](TEST_ARCHITECTURE.md) | 测试分层 |

## architecture / runtime 子目录

- [architecture/runtime_v1_audit.md](architecture/runtime_v1_audit.md) — 历史架构审计
- [runtime/world_runtime_interface.md](runtime/world_runtime_interface.md) — World 运行时接口笔记
