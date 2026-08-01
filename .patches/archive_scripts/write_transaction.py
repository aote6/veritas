content = '''# Veritas Transaction Specification v0.1

## 1. Transaction 是什么

Transaction 是 Veritas 中状态变更的基本单位。

不是数据库事务。不是 Rust 的函数调用。Transaction 是 Veritas
Machine 执行指令时的一种执行边界，保证 ACID 属性。

## 2. Transaction 的边界

指令 BEGIN 和 COMMIT / ABORT 界定一个 Transaction:

BEGIN
  ... 指令序列 ...
COMMIT  (或 ABORT)

Transaction 不可嵌套。一个 Transaction 未结束时，不可开始
新的 Transaction。

## 3. Transaction 的属性

### 3.1 原子性

Transaction 内的所有操作要么全部生效，要么全部不生效。
COMMIT 时所有副作用一次性应用。ABORT 时所有副作用回滚。

### 3.2 一致性

Transaction 提交后，Veritas 世界从一个一致状态转移到
另一个一致状态。Object 的生命周期规则、Capability 树的
完整性、Link 的语义约束，在 Transaction 前后都保持。

### 3.3 隔离性

Transaction 之间相互隔离。通过 Snapshot Isolation 实现:
- 每个 Transaction 在 BEGIN 时获得 MemorySpace 的一致性快照
- Transaction 内读取的是快照版本
- Transaction 内的写入暂存在私有 WriteSet 中
- COMMIT 时检测写写冲突

### 3.4 持久性

已提交的 Transaction 通过 WAL 持久化。崩溃恢复时从 WAL
重放已提交的事务，撤销未提交的事务。

## 4. Transaction 的执行流程

1. BEGIN: Machine 记录当前全局版本号，创建 TransactionContext
2. 执行指令: 读取从快照读取，写入暂存在 WriteSet
3. COMMIT:
   a. 冲突检测: 检查读取过的 MemorySpace 版本是否变化
   b. 如果冲突: ABORT (reason = CONFLICT)
   c. 如果无冲突: WriteSet 合并到 MemorySpace，版本号递增
   d. 副作用应用: Object 创建/删除生效，Capability 变更生效，
      Link 变更生效
   e. WAL 写入
   f. Effect 执行
4. ABORT: WriteSet 丢弃，所有暂存操作回滚

## 5. 冲突检测

在 COMMIT 时执行:

- 遍历当前 Transaction 的 ReadSet
- 对每个 (ObjectId, StateId)，检查 MemorySpace 中的当前版本
- 如果当前版本高于 Transaction 开始时的版本，说明有其他
  Transaction 修改了该数据
- 冲突时 ABORT，reason = CONFLICT

## 6. 写写冲突

两个并发 Transaction 写入同一个 (ObjectId, StateId):
- 先 COMMIT 的成功
- 后 COMMIT 的检测到冲突，ABORT
- 这是乐观并发控制的策略

## 7. Transaction 与 Object

- OBJECT_BIRTH 在 Transaction 内
- OBJECT_DEATH 在 Transaction 内
- OBJECT_LINK 在 Transaction 内
- OBJECT_UNLINK 在 Transaction 内
- OBJECT_FREEZE 在 Transaction 内
- MemorySpace 的修改在 Transaction 内

Transaction 提交后:
- 新 Object 进入 ACTIVE
- 死亡 Object 进入 DEAD
- Link 建立
- MemorySpace 版本更新

Transaction 中止后:
- 所有操作回滚，世界状态不变

## 8. Transaction 与 Effect

Effect 是 Transaction 提交后需要执行的外部副作用:

- Effect 在 WAL 写入后执行
- Effect 执行失败不导致 Transaction 回滚
- Effect 通过 idempotency_key 保证幂等性
- 崩溃恢复时，已提交但 Effect 未确认的事务需要重试 Effect

## 9. Savepoint

Transaction 内可以设置 Savepoint:

SAVEPOINT <id>

ROLLBACK_TO <id>:
- 回滚到指定 Savepoint 的状态
- Savepoint 之后的写入被丢弃
- Savepoint 之后的 Object 创建/删除回滚
- Savepoint 之后的 Link 操作回滚

## 10. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| BEGIN/COMMIT/ABORT | engine.rs | 转为 ISA 指令 |
| Snapshot Isolation | state_memory.rs + engine.rs | 保持 |
| 冲突检测 | engine.rs detect_conflict | 移到 Machine 层 |
| WAL | wal.rs | 保持 |
| Savepoint | engine.rs savepoint | 保持 |

## 11. 实现要求

1. Transaction 不可嵌套
2. 隔离级别为 Snapshot Isolation
3. 冲突检测在 COMMIT 时执行
4. 所有状态变更必须事务性
5. Effect 必须幂等
'''

with open('docs/constitution/transaction.md', 'w') as f:
    f.write(content)
print('Done: docs/constitution/transaction.md')
