# TRAP ABI 冻结文档

**日期**: 2026-08-20
**状态**: 设计冻结，未改代码
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


### 1.2 错误返回方案

**选用：TrapResult 增加 Error 变体**

当前 TrapResult 只有 Success 和几种 ID 变体，错误通过裸 Err 向上抛。这导致：
- 权限失败时 Machine 直接 panic
- 错误信息无法写回寄存器
- TRAP 和旧指令的失败形态不一致

增加：
- TrapResult::Error { code: u8 }，code 对应 TrapReason
- Machine 捕获 Error 后设置 status = Trapped(reason)

这样 TRAP 和旧指令在失败时都能优雅终止，不再裸 Err。


### 1.3 service_id 分配

当前已分配 0-8，但 CapabilityGrant/Revoke/Delegate/MemoryAlloc 缺失。

**冻结以下分配：**

| service_id | KernelCall | 参数 ABI | 返回 |
|---|---|---|---|
| 0 | ObjectBirth | R0=object_type | ObjectId 到 R0 |
| 1 | ObjectDeath | R0=object_id | Success |
| 2 | ObjectLink | R0=from, R1=to, R2=link_type | Success |
| 3 | ObjectUnlink | R0=from, R1=to | Success |
| 4 | ObjectFreeze | R0=object_id | Success |
| 5 | Commit | 无 | Success |
| 6 | Effect | R0=内存指针 | EffectKey 到 R0 |
| 7 | Savepoint | R0=内存指针 | Success |
| 8 | RollbackTo | R0=内存指针 | Success |
| 9 | CapabilityGrant | R0=内存指针 | CapabilityId 到 R0 |
| 10 | CapabilityRevoke | R0=内存指针 | Success |
| 11 | CapabilityDelegate | R0=内存指针 | Success |
| 12 | MemoryAlloc | R0=object_id, R1=size_hint | Success |


## 2. 内存参数块格式（为复杂服务预留）

### 2.1 通用格式

当 service_id 需要内存参数时，R0 指向以下布局：

偏移 0: u16 参数块长度（字节）
偏移 2: u8  参数数量
偏移 3: 参数字段起始

### 2.2 CapabilityGrant 参数块（service_id 9）

偏移 0: u16 总长度
偏移 2: u8 参数数量（4）
偏移 3: u64 grantor
偏移 11: u64 grantee
偏移 19: u16 capability_type 字符串长度
偏移 21: u8[] capability_type 字符串字节
偏移 21+n: u64 resource

### 2.3 Effect 参数块（service_id 6）

偏移 0: u16 总长度
偏移 2: u8 参数数量（1）
偏移 3: u32 payload 长度
偏移 7: u8[] payload 字节


## 3. 错误码映射

| TrapResult::Error code | TrapReason |
|---|---|
| 1 | AccessDenied |
| 2 | InvalidEncoding |
| 3 | MemoryFault |
| 4 | WriteConflict |
| 5 | PermissionDenied |
| 6 | ObjectNotFound |
| 7 | ObjectNotAlive |


## 4. 迁移顺序

### 批次 0（已完成）
- ObjectBirth、Commit 的 TRAP 等价性已验证

### 批次 1（下一步）
- 补全 ObjectDeath、ObjectUnlink 的等价性测试
- 验证 TrapResult::Error 变体的行为一致性

### 批次 2（复杂参数）
- 实现内存参数块读取
- 迁移 CapabilityGrant、Effect、Savepoint

### 批次 3（退役旧指令）
- 所有 KernelCall 都有 TRAP 路径后，删除 Instruction 中的旧式变体


## 5. 本轮不做的

1. 不修改任何生产代码
2. 不实现内存参数块读取
3. 不增加 TrapResult::Error 变体
4. 不删除旧式指令
5. 不改变现有 service_id 0-8 的 ABI


## 6. 已确认的等价性测试结果

- ObjectBirth: TRAP 0 == OBJECT_BIRTH 通过
- ObjectFreeze: TRAP 4 == OBJECT_FREEZE 通过
- Commit: TRAP 5 == COMMIT 通过
- ObjectLink: TRAP 2 与旧式 KernelCall 结构等价 通过


**冻结日期**: 2026-08-20
**冻结状态**: ABI 设计已冻结，实现待批次 2 开始
