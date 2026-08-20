# TRAP ABI Error Contract Freeze

**日期**: 2026-08-20
**状态**: Phase 1 审计完成，等待代码落地
**范围**: VeritasError 到 TrapResult::Error 到 TrapReason 的完整映射链


## 1. 完整错误码表

### 1.1 VeritasError 到 ABI Error Code

| ABI code | 常量名 | VeritasError | 说明 |
|---|---|---|---|
| 1 | TRAP_ERR_ACCESS_DENIED | 预留 | Machine CALL 拒绝时使用 |
| 2 | TRAP_ERR_ENGINE | EngineError(String) | 引擎内部错误 |
| 3 | TRAP_ERR_MEMORY_FAULT | 预留 | 暂未使用，等真实内存错误 |
| 4 | TRAP_ERR_WRITE_CONFLICT | Abort(WriteConflict) | 写冲突 |
| 4 | TRAP_ERR_WRITE_CONFLICT | Abort(ReadFutureVersion) | 读未来版本，也属于冲突 |
| 4 | TRAP_ERR_WRITE_CONFLICT | Abort(AlreadyAborted) | 事务已中止 |
| 4 | TRAP_ERR_WRITE_CONFLICT | Abort(PhantomConflict) | 幻影冲突 |
| 4 | TRAP_ERR_WRITE_CONFLICT | DeterminismViolation | 确定性违规 |
| 5 | TRAP_ERR_PERMISSION_DENIED | PermissionDenied | 权限拒绝 |
| 6 | TRAP_ERR_STATE_NOT_FOUND | Abort(StateNotFound) | 状态不存在 |


### 1.2 ABI Error Code 到 TrapReason

| ABI code | TrapReason | 说明 |
|---|---|---|
| 1 | AccessDenied { pc } | 权限拒绝 |
| 2 | EngineError { pc } | 引擎错误，需要新增此变体 |
| 3 | 不映射 | 预留，暂不使用 |
| 4 | WriteConflict { pc } | 写冲突，需要新增此变体 |
| 5 | AccessDenied { pc } | 权限拒绝 |
| 6 | StateNotFound { pc } | 状态不存在，需要新增此变体 |
| 未知 | UnknownKernelError { code, pc } | 需要新增此变体 |


## 2. 需要的代码改动

### 2.1 src/types.rs 的 TrapReason 新增变体

- EngineError { pc: usize }
- WriteConflict { pc: usize }
- StateNotFound { pc: usize }
- UnknownKernelError { code: u8, pc: usize }

### 2.2 src/kernel.rs 定义错误码常量

在 TrapResult 附近新增：

pub const TRAP_ERR_ACCESS_DENIED: u8 = 1;
pub const TRAP_ERR_ENGINE: u8 = 2;
pub const TRAP_ERR_MEMORY_FAULT: u8 = 3;
pub const TRAP_ERR_WRITE_CONFLICT: u8 = 4;
pub const TRAP_ERR_PERMISSION_DENIED: u8 = 5;
pub const TRAP_ERR_STATE_NOT_FOUND: u8 = 6;

### 2.3 src/kernel.rs 的 from_error 使用常量

把裸数字换成常量名。

### 2.4 src/machine.rs 的 map_trap_code 对齐 TrapReason

严格按 1.2 表格映射，未知 code 使用 UnknownKernelError。

### 2.5 新增 ABI 测试

在 tests/trap_equivalence.rs 或新文件 tests/trap_error_abi.rs 中验证：

- 每个 VeritasError 映射到正确的 code
- 每个 code 映射到正确的 TrapReason
- 未知 code 映射到 UnknownKernelError
- WriteConflict 不等于 IllegalInstruction
- StateNotFound 不等于 InvalidEncoding


## 3. 明确不做的事

1. 不实现 MemoryFault 的真实 addr/size 传递
2. 不实现 IllegalInstruction 的 Kernel 到 Machine 传递
3. 不引入 Error { code, detail } 结构
4. 不修改 WorldService 的 Result 签名
5. 不进入 Batch 2 内存参数块


## 4. 冻结判定标准

以下条件全部满足后，Error ABI 才算冻结：

- 所有 VeritasError 变体都有明确 code
- 所有 code 都有明确 TrapReason
- 未知 code 不再被静默映射成 InvalidEncoding
- ABI 测试覆盖完整映射链
- 全量测试通过

## 6. Code 1 vs Code 5 语义边界（2026-08-20 补充）

**code 1 = TRAP_ERR_ACCESS_DENIED**
- 层级：Machine CALL 指令层级的访问拒绝
- 来源：未来 Machine 执行 CALL 时的硬件级访问检查
- 当前状态：预留，尚无实际产生来源

**code 5 = TRAP_ERR_PERMISSION_DENIED**
- 层级：Kernel 层级的权限拒绝
- 来源：VeritasError::PermissionDenied
- 当前状态：活跃使用

两者都映射到 TrapReason::AccessDenied，但保留不同的 ABI code 是有意设计：
- 便于未来区分错误来源层级
- 避免语义耦合：Machine 层拒绝不应伪装成 Kernel 层拒绝

**冻结判定**：两个 code 都保留，不合并。

## 7. 测试闭环状态（2026-08-20 更新）

- `tests/trap_error_abi.rs`：验证 VeritasError → TrapResult::Error(code)
  - 6 个测试，覆盖所有 VeritasError 变体到 ABI code 的映射
- `src/machine.rs` unit tests（trap_mapping_tests）：验证 TrapResult::Error(code) → 真实 map_trap_code() → TrapReason
  - 8 个测试，直接调用生产代码，无镜像实现
- 两部分测试共同构成完整映射链的闭环
