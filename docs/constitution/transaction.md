# Veritas Transaction Specification v0.2

## 1. Transaction 是什么

Transaction 是 Veritas Machine 中状态变更的基本单位，同时承载
执行上下文。

不是数据库事务。不是 Rust 函数调用。Transaction 是 Veritas 的
第五个核心原语，地位与 Object、Memory、Kernel、Module 同等。

## 2. Transaction 的双重角色

Transaction 承担两个职责:

### 2.1 状态变更边界
- 所有对 Object 和 Memory 的修改都在 Transaction 内
- BEGIN 开始，COMMIT 或 ABORT 结束
- 原子性: 所有修改要么全部生效，要么全部不生效
- 一致性快照: 整个 Transaction 看到同一个快照版本
- 隔离性: Snapshot Isolation，冲突检测在 COMMIT 时执行
- 持久性: COMMIT 后写入 WAL，崩溃可恢复

### 2.2 执行上下文容器
- Transaction 持有当前执行状态: registers、pc、current_object
- CALL 在同一个 Transaction 内切换 current_object
- RETURN 恢复到调用者的执行上下文

## 3. Transaction 的生命周期

状态机: BEGIN -> ACTIVE -> COMMITTED | ABORTED

- BEGIN: 创建 TransactionContext，记录快照版本号
- ACTIVE: 执行指令，暂存写入，持有执行上下文
- COMMITTED: 冲突检测通过，副作用应用，WAL 写入，不可逆
- ABORTED: 所有副作用回滚，世界状态不变

不可嵌套。一个 Transaction 未结束时不可开始新的 Transaction。

## 4. Execution Context

Transaction 持有完整的执行上下文:

TransactionContext:
- snapshot_version: BEGIN 时的全局版本号
- current_object: 当前正在执行的 ObjectId
- pc: 程序计数器
- registers: 通用寄存器组
- call_stack: CallFrame 列表，CALL 时压入，RETURN 时弹出
- read_set: Transaction 内读取过的所有 Address
- write_set: Transaction 内修改的所有 Address 和新值
- tx_id: TransactionId
- state: ACTIVE | COMMITTED | ABORTED
- capability_context: 以哪个 Object 的身份做 Capability 检查

CallFrame:
- caller_object: 调用者 ObjectId
- return_pc: 返回地址
- return_registers: 调用者寄存器快照

注意: call_stack 属于 TransactionContext，不属于 Machine。

## 5. current_object 的归属

current_object 属于 Transaction，不属于 Machine。

- Machine 可以执行多个 Transaction（串行，不是并发）
- 每个 Transaction 有自己的 current_object
- CALL 指令在 Transaction 内切换 current_object
- RETURN 指令从 call_stack 恢复 current_object
- Transaction 结束时 current_object 自然释放

Machine 本身不持有 current_object。current_object 只在执行
Transaction 时有意义。

## 6. 读写集的键

Transaction 内所有读写集、冲突检测、MemorySpace 寻址统一使用
Address。

Address = (ObjectId, StateId)

- 全局唯一，天然携带 Object 边界
- ReadSet 记录 Transaction 内读取过的所有 Address
- WriteSet 记录 Transaction 内修改的所有 Address 和新值
- 冲突检测按 Address 比较版本号
- MemorySpace 的寻址也使用 Address

使用 Address 而不是裸 StateId:
- StateId 不携带 Object 信息，无法区分不同 Object 的同名 State
- Address 直接支持跨 Object Transaction 的冲突检测
- Address 与 Memory 的寻址方式一致
- 避免 ReadSet 用 Address、WriteSet 用 StateId 的半升级状态

## 7. 跨 Object Transaction

一个 Transaction 内可以访问和修改多个 Object 的 MemorySpace。

- CALL 指令切换 current_object，但不开始新 Transaction
- 被调用 Object 的代码在同一个 Transaction 内执行
- 读写集统一记录所有 Object 的修改
- COMMIT 时，所有 Object 的修改一起生效
- ABORT 时，所有 Object 的修改一起回滚

跨 Object 的变更可以保持原子性。

## 8. CALL 与 Transaction 的关系

CALL 指令:
1. 保存当前 CallFrame 到 call_stack
2. 设置 current_object = 目标 ObjectId
3. 设置 pc = 目标入口偏移
4. Transaction 不变（同一个 Transaction）

RETURN 指令:
1. 从 call_stack 弹出 CallFrame
2. 恢复 current_object = caller_object
3. 恢复 pc = return_pc
4. 恢复 registers = return_registers
5. Transaction 不变

CALL 和 RETURN 不改变 Transaction 的边界。

## 9. Snapshot 语义

- BEGIN 时记录全局版本号作为 snapshot_version
- Transaction 内读取 Memory 时:
  - 优先从 write_set 读取（读自己的写入）
  - 否则从 MemorySpace 读取，检查 slot 版本 <= snapshot_version
  - 若 Address 当前已提交版本大于 snapshot_version，
    则该 Address 在当前 Transaction 看来发生了并发更新
- COMMIT 时:
  - 遍历 read_set，检查每个 Address 的当前已提交版本
  - 如果任何 Address 的当前版本 > snapshot_version，冲突，ABORT
  - 冲突检测的粒度是 Address，不是整个 Object

Snapshot 的边界是 Transaction，不是 Object。
一个 Transaction 内部看到的是同一个一致性快照，无论跨多少 Object。

## 10. Rollback 语义

- ABORT 回滚整个 Transaction
- 所有 Object 的写入回滚（不是只回滚当前 Object）
- 所有 Object 的创建回滚
- 所有 Link 操作回滚
- 所有 Capability 操作回滚
- call_stack 清空
- 执行上下文失效

## 11. Savepoint（未来扩展）

SAVEPOINT 和 ROLLBACK_TO:
- 部分回滚 Transaction 内的操作
- Savepoint 记录当时的 write_set 和 pending_objects 状态
- ROLLBACK_TO 回滚到 Savepoint 的状态
- call_stack 恢复到 Savepoint 时的状态

注意: Savepoint 是未来扩展，当前版本未实现。
当前版本仅支持完整的 COMMIT 或 ABORT。

## 12. Transaction 与 Kernel

- TRAP 指令在 Transaction 内执行
- 内核服务使用当前 Transaction 的上下文
- 内核服务的副作用暂存在 write_set 和 pending_objects 中
- COMMIT 时内核服务副作用应用
- ABORT 时内核服务副作用回滚

## 13. 确定性

- 同样的初始状态 + 同样的指令序列 = 同样的 Transaction 结果
- TransactionId 确定性生成（基于 tx_id_counter，不是随机数）
- COMMIT 后的状态完全可重现
- 这是 Replay 和 Receipt 验证的基础

## 14. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| TransactionContext | types.rs TransactionContext | 已实现 |
| ReadSet/WriteSet | engine.rs | 键已改为 Address（P24） |
| current_object | machine.rs ctx.current_object | 属于 TransactionContext |
| call_stack | machine.rs Vec<CallFrame> | 已实现 |
| 跨 Object 事务 | machine.rs Call/Return | 同一 tx 内切换 current_object |
| Snapshot | state_memory.rs | 保持 |
| Savepoint | engine.rs savepoint | 保持现状，未来扩展 |

## 15. 实现要求

1. Transaction 不可嵌套
2. 读写集键为 Address = (ObjectId, StateId)
3. current_object 属于 Transaction，不属于 Machine
4. CALL 不创建新 Transaction，在同一个 Transaction 内切换
5. ABORT 回滚整个 Transaction，不是只回滚当前 Object
6. 冲突检测在 COMMIT 时执行，粒度为 Address
