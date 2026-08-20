# TRAP Kernel Service Entrypoint Closure

**日期**: 2026-08-20  
**基线**: 8d2b22a + Batch 2  

## 裁定摘要

### Abort → service_id 13（已实现）

- KernelCall::Abort 真实存在，经 `engine.abort`
- R0 = reason_tag (0..4)
- TRAP 成功后 Machine 设置 `MachineStatus::Aborted`（与 Instruction::Abort 对齐）
- 非法 tag → InvalidEncoding

### HostCall → **不**纳入 TRAP（情况 B）

- 定义：`src/host.rs`，外部宿主能力
- 不进入 Kernel / KernelCall
- 独立 `Instruction::HostCall { call_id }`
- 与 Kernel service_id 数字空间故意分离

### 旧式 Kernel Instruction → **保留兼容**（本轮不删除）

阻塞退役原因：

1. tests/machine、programs/*.vasm、examples、trap_equivalence 仍使用 OBJECT_BIRTH/COMMIT 等
2. Assembler 仍生成旧助记符
3. Instruction::ObjectBirth 含 attach_capability Machine 胶水（TRAP 路径已对齐）
4. Effect 旧路径不写 EffectKey 到 R0；TRAP 写 R0（ABI 增强，非 bug）

正式入口：`Instruction::Trap` → `KernelCall::decode_with_memory` → `Kernel::handle`

### Machine 原生指令 → 不 TRAP 化

Read/Write/算术/跳转/Call/Return/Nop/Halt 等保持 Machine 执行语义。

## service_id 完整表

| id | KernelCall | ABI |
|----|------------|-----|
| 0–5 | Birth/Death/Link/Unlink/Freeze/Commit | 寄存器 |
| 6–11 | Effect/Savepoint/Rollback/Grant/Revoke/Delegate | 参数块 |
| 12 | MemoryAlloc | 寄存器 |
| 13 | Abort | 寄存器 R0=reason |

## 不变量

1. Kernel service 正式 Machine ABI = TRAP  
2. TRAP 只 decode + handle（Abort/ObjectBirth 的 Machine 后处理仅对齐生命周期与 CALL 授权）  
3. HostCall ∉ KernelCall  
4. 非法 ABI fail-closed  
