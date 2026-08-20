# TRAP ABI Error Contract Audit

**日期**: 2026-08-20
**状态**: 审计完成，待冻结
**范围**: TrapResult::Error 的错误码定义、Machine 映射、TrapFrame 语义


## 1. 现状事实

### 1.1 Kernel 层错误码定义（src/kernel.rs:148-165）

| code | VeritasError 来源 | 含义 |
|---|---|---|
| 5 | PermissionDenied | 权限拒绝 |
| 4 | Abort(WriteConflict) | 写冲突 |
| 4 | Abort(ReadFutureVersion) | 读未来版本 |
| 4 | Abort(AlreadyAborted) | 已中止 |
| 6 | Abort(StateNotFound) | 状态不存在 |
| 4 | Abort(PhantomConflict) | 幻影冲突 |
| 2 | EngineError(String) | 引擎错误 |
| 4 | DeterminismViolation | 确定性违规 |

### 1.2 Machine 层 TrapReason 映射（src/machine.rs:20-31）

| code | TrapReason | 是否丢失 detail |
|---|---|---|
| 1 | AccessDenied { pc } | 无 detail 可丢 |
| 2 | InvalidEncoding { pc } | EngineError 的字符串丢了 |
| 3 | MemoryFault { addr: 0, size: 0 } | addr/size 硬编码 0 |
| 4 | IllegalInstruction { opcode: 0 } | opcode 硬编码 0 |
| 5 | AccessDenied { pc } | 无 detail 可丢 |
| 6 | InvalidEncoding { pc } | StateNotFound 被归类为编码错误 |
| 7 | InvalidEncoding { pc } | 未使用 |
| unknown | InvalidEncoding { pc } | 未知 code 被归类为编码错误 |


## 2. 审计问题

### 2.1 Trap ABI 需要错误类别还是完整错误信息？

**初步判断**：类别足够。

理由：
- Machine 收到 Error 后只做一件事：设置 Trapped 状态并停止
- Machine 不需要把 VeritasError 的字符串展示给用户
- 完整错误信息由 Kernel 日志或 WorldService 负责

### 2.2 当前映射的语义损失点

1. **code 3 的 addr/size 硬编码 0**：如果未来有真实 MemoryFault，这里会制造假数据
2. **code 4 的 opcode 硬编码 0**：IllegalInstruction 无法区分具体哪个 opcode
3. **code 6 StateNotFound 映射到 InvalidEncoding**：语义不准确，StateNotFound 不是编码问题
4. **code 2 EngineError(String) 的字符串丢了**：如果能保留字符串，诊断会更容易

### 2.3 未知 code 的行为

当前：未知 code 映射到 InvalidEncoding { pc }

问题：如果 Kernel 未来增加新错误码，Machine 会静默把它当 InvalidEncoding，而不是显式报未知错误码。


## 3. 设计建议（供讨论）

### 方案 A：保持 TrapResult::Error(u8)，但收紧映射

- code 3 改为不映射到 MemoryFault，因为 Veritas 没有真实内存错误来源
- code 4 改为不映射到 IllegalInstruction，因为 WriteConflict 不是非法指令
- code 6 改为 AccessDenied 或新增 TrapReason::StateNotFound

### 方案 B：TrapResult::Error 携带可选 detail

结构改为：Error { code: u8, detail: u64 }

- 简单错误不用 detail
- MemoryFault 用 detail 传 addr
- EngineError 无法传字符串，因为 u64 不够

### 方案 C：Machine 不映射，直接保留 code

TrapReason 增加：KernelError { code: u8, pc: usize }

Machine 不再把 code 映射成语义化 TrapReason，而是保留原始 code。

**优点**：不再丢失信息，不再伪造 detail
**缺点**：TrapReason 的语义化程度降低


## 4. 待冻结决策

| 问题 | 选项 | 倾向 |
|---|---|---|
| Trap ABI 需要类别还是完整信息？ | 类别 | 类别 |
| code 3 如何处理？ | 删除或保留为预留 | 删除，直到有真实来源 |
| code 4 如何处理？ | 映射到 WriteConflict 语义 | 新增 TrapReason::WriteConflict |
| code 6 如何处理？ | 映射到 StateNotFound 语义 | 新增 TrapReason::StateNotFound |
| 未知 code 如何处理？ | 显式报错 | 显式报错 |
| Machine 是否保留原始 code？ | 是 | 是 |


## 5. 结论

当前 Error ABI 已基本工作，但映射表有 4 处语义损失需要修复。

**下一步**：确认上面 6 个决策后，更新 src/machine.rs 的 map_trap_code 和 src/types.rs 的 TrapReason 枚举，然后跑全量测试。
