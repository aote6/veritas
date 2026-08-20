# TRAP ABI 冻结文档

**日期**: 2026-08-20
**状态**: 设计冻结 + Batch 2 实现落地
**目的**: 为 Machine 到 Kernel Service 的 TRAP 入口定义完整、可扩展的 ABI，作为后续迁移的唯一依据


## 1. 核心决策

### 1.1 参数传递方案

**选用：混合寄存器 + 内存块**

- 简单参数（ObjectId、Version、u8、u64）：继续用 R0/R1/R2
- 复杂参数（String、Vec<u8>、CapabilityGrant 的多参）：R0 指向内存中的参数块

理由：
- 简单服务不受影响，保持现有等价性测试的 ABI
- 复杂服务有足够空间，不需要为每个服务设计专用寄存器布局
- 内存块布局可以在不改变指令集的前提下扩展

**Endianness**: 所有参数块中的多字节整数字段均为 **little-endian**。


### 1.2 错误返回方案

**选用：TrapResult 增加 Error 变体**

- TrapResult::Error(code: u8)，code 对应 TrapReason
- Machine 捕获 Error 后设置 status = Trapped(reason)
- 参数块 decode 失败（OOB、非法 UTF-8、非法 tag、长度不一致）→ Machine 直接 `Trapped(InvalidEncoding)`


### 1.3 service_id 分配

| service_id | KernelCall | 参数 ABI | 返回 |
|---|---|---|---|
| 0 | ObjectBirth | R0=object_type | ObjectId → R0 |
| 1 | ObjectDeath | R0=object_id | Success |
| 2 | ObjectLink | R0=from, R1=to, R2=link_type | Success |
| 3 | ObjectUnlink | R0=from, R1=to | Success |
| 4 | ObjectFreeze | R0=object_id | Success |
| 5 | Commit | 无 | Success |
| 6 | Effect | R0=内存指针（参数块） | EffectKey → R0（UTF-8 bytes） |
| 7 | Savepoint | R0=内存指针（参数块） | Success |
| 8 | RollbackTo | R0=内存指针（参数块） | Success |
| 9 | CapabilityGrant | R0=内存指针（参数块） | CapabilityId → R0 |
| 10 | CapabilityRevoke | R0=内存指针（参数块） | Success |
| 11 | CapabilityDelegate | R0=内存指针（参数块） | Success |
| 12 | MemoryAlloc | R0=object_id, R1=size_hint | StateId → R0 |
| 13 | Abort | R0=reason_tag | MachineStatus::Aborted |


## 2. 内存参数块格式

### 2.1 通用格式

当 service_id 需要内存参数时，R0 指向以下布局（little-endian）：

```
偏移 0: u16 参数块总长度 total_len（字节，含本 header）
偏移 2: u8  参数数量 field_count
偏移 3: 参数字段起始
```

严格约束：
- 整个块必须落在 Machine RAM 内
- `total_len` 必须与字段实际占用完全一致（不允许尾部多余数据）
- 失败一律 fail-closed（不截断、不默认空值）


### 2.2 Effect 参数块（service_id 6）

```
偏移 0: u16 total_len
偏移 2: u8  field_count = 1
偏移 3: u32 payload_len
偏移 7: u8[payload_len] payload
```

约束：`total_len == 7 + payload_len`，`field_count == 1`。允许 `payload_len == 0`。


### 2.3 Savepoint 参数块（service_id 7）

```
偏移 0: u16 total_len
偏移 2: u8  field_count = 1
偏移 3: u16 name_len
偏移 5: u8[name_len] UTF-8 name
```

约束：`total_len == 5 + name_len`，`field_count == 1`。允许 `name_len == 0`。name 必须合法 UTF-8。


### 2.4 RollbackTo 参数块（service_id 8）

与 Savepoint 完全对称（同上布局与约束）。


### 2.5 CapabilityGrant 参数块（service_id 9）

```
偏移 0:  u16 total_len
偏移 2:  u8  field_count = 4
偏移 3:  u64 grantor
偏移 11: u64 grantee
偏移 19: u16 capability_type_len
偏移 21: u8[capability_type_len] capability_type（UTF-8）
偏移 21+n: u64 resource
```

其中 `n = capability_type_len`。
约束：`total_len == 21 + n + 8`，`field_count == 4`。capability_type 必须合法 UTF-8。

注：旧式 `Instruction::CapabilityGrant` 的 grantor 固定为 `ctx.current_object`；TRAP 允许显式 grantor。


### 2.6 CapabilityRevoke 参数块（service_id 10）

```
偏移 0:  u16 total_len
偏移 2:  u8  field_count = 3
偏移 3:  u64 capability_id
偏移 11: u64 holder
偏移 19: u8  cascade_tag
```

`cascade_tag`：
- `0` → `None`
- `1` → `Some(false)`
- `2` → `Some(true)`
- 其他 → 非法，拒绝

约束：`total_len == 20`，`field_count == 3`。


### 2.7 CapabilityDelegate 参数块（service_id 11）

```
偏移 0:  u16 total_len
偏移 2:  u8  field_count = 4
偏移 3:  u64 capability_id
偏移 11: u64 from
偏移 19: u64 to
偏移 27: u8  cascade_on_revoke
```

`cascade_on_revoke`：`0` = false，`1` = true，其他非法。

约束：`total_len == 28`，`field_count == 4`。


### 2.8 MemoryAlloc（service_id 12）

**不使用参数块**。寄存器 ABI：

- R0 = object_id
- R1 = size_hint

返回：`TrapResult::StateId` → 写入 R0。



### 2.9 Abort（service_id 13）

寄存器 ABI（无参数块）：

- R0 = reason_tag
  - 0 WriteConflict
  - 1 ReadFutureVersion
  - 2 AlreadyAborted
  - 3 StateNotFound
  - 4 PhantomConflict
  - 其他值非法 → decode 失败 → InvalidEncoding

Kernel 侧：`engine.abort(ctx, reason)`。

Machine 侧（TRAP 后处理，与 `Instruction::Abort` 对齐）：成功后设置 `MachineStatus::Aborted(reason)`。

Abort 属于 Kernel service（事务中止语义）+ Machine 生命周期控制；通过 TRAP 到达时 Machine 必须执行 status 转换。


## 2.10 HostCall 架构裁定（非 Kernel service）

`Instruction::HostCall { call_id }` **不属于** KernelCall / TRAP service domain。

依据 `src/host.rs`：

> Host Calls are provided by the external environment, not by Kernel mode.

合法 call_id：0 Time / 1 Random / 2 Write / 3 Read / 4 Spawn。

未知 call_id → `TrapReason::InvalidEncoding`。

**不得**将 HostCall 映射为 TRAP service_id（与 Kernel 0–13 数字空间冲突且语义层级不同）。


## 2.11 旧式 Kernel Instruction（兼容层，尚未退役）

以下仍可执行，语义应经 `KernelCall::handle` 或等价 pub(crate) 转发，不得拥有独立 Engine 语义：

ObjectBirth, ObjectDeath, ObjectLink, ObjectUnlink, ObjectFreeze, Commit,
Effect, Savepoint, RollbackTo, CapabilityGrant, Abort

退役条件（全部满足后方可删除）：Machine E2E 等价性、无内部依赖、assembler/tests/examples 已切换。

当前：**保留兼容入口**；正式 ABI 入口为 TRAP。


## 3. 错误码映射

以 `docs/TRAP_ABI_ERROR_CONTRACT_FREEZE.md` 与源码常量为准：

| TrapResult::Error code | 常量 | TrapReason |
|---|---|---|
| 1 | TRAP_ERR_ACCESS_DENIED | AccessDenied |
| 2 | TRAP_ERR_ENGINE | EngineError |
| 3 | TRAP_ERR_MEMORY_FAULT | UnknownKernelError（预留） |
| 4 | TRAP_ERR_WRITE_CONFLICT | WriteConflict |
| 5 | TRAP_ERR_PERMISSION_DENIED | AccessDenied |
| 6 | TRAP_ERR_STATE_NOT_FOUND | StateNotFound |
| 其他 | — | UnknownKernelError |

参数块 ABI 解码失败不走 Kernel handle，由 Machine 直接 `InvalidEncoding`。


## 4. 迁移顺序

### 批次 0–1（已完成）
- 简单 KernelCall 0–5 TRAP 路径与等价性
- TrapResult::Error 映射闭环

### 批次 2（本轮）
- 参数块 decoder（6–11）+ MemoryAlloc（12）
- Machine `decode_with_memory` 统一入口
- EffectKey → R0
- malformed fail-closed
- Kernel/TRAP 语义等价性测试

### 批次 3（未开始）
- 所有 KernelCall 均有 TRAP 路径且等价性通过后，删除旧式 Instruction 变体


## 5. 实现入口

```
Instruction::Trap { service_id }
    → KernelCall::decode_with_memory(service_id, r0, r1, r2, &ram)
    → Kernel::handle(&mut ctx, call)
    → TrapResult → R0 / MachineStatus::Trapped
```

`KernelCall::decode` 仍仅支持寄存器服务（0–5、12）；复杂服务必须用 `decode_with_memory`。


**冻结日期**: 2026-08-20
**Batch 2 落地**: 2026-08-20
