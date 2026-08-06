# Veritas Memory Specification v0.1

## 1. Memory 是什么

Memory 是 Veritas Machine 中所有可寻址状态的容器。

不是 Rust 的 Vec<u8>。不是全局 HashMap。Memory 是 Veritas 的
一等原语，与 Object 绑定，通过 ISA 指令访问。

## 2. Memory 的结构

Veritas Memory 不是单一线性地址空间。它是多个 Memory Space 的集合，
每个 Memory Space 属于一个 Object。

Memory = { ObjectId -> MemorySpace }

全局视角不存在"一块大内存"。Machine 执行指令时，根据当前上下文
确定操作哪个 Object 的 Memory Space。

## 3. Memory Space 的内部结构

MemorySpace:
- slots: 状态槽位的有序列表
- version: 单调递增的版本号
- owner: 所属 Object 的 ObjectId

每个槽位:
- key: StateId (64位无符号整数)
- value: 字节序列
- version: 该槽位最后被写入时的 MemorySpace 版本号

## 4. 寻址方式

地址 = (ObjectId, StateId)

没有全局地址。Machine 执行 STORE/LOAD 时:
1. 从当前执行上下文获取目标 ObjectId
2. 通过 Capability 验证访问权限
3. 在目标 Object 的 Memory Space 中查找 StateId

## 5. 读写语义

READ:
- 输入: ObjectId, StateId
- 输出: 当前 Transaction 可见的 (value, version)
- 可见性由 Transaction 的隔离级别决定

WRITE:
- 输入: ObjectId, StateId, new_value
- 写入数据暂存在当前 Transaction 的 WriteSet
- Transaction 提交时写入 Memory Space，版本号递增
- Transaction 中止时不写入

## 6. 版本管理

- 每个 Memory Space 有一个全局版本号
- 每次提交写入后，Memory Space 版本号递增
- 每个槽位记录自己最后一次被写入时的版本号
- Transaction 读取时检查版本号，检测冲突

## 7. 与 Transaction 的关系

- 读取: 通过 Snapshot Isolation 获取一致性快照
- 写入: 暂存在 Transaction 私有 WriteSet 中
- 提交: WriteSet 合并到 Memory Space，版本号递增
- 中止: WriteSet 丢弃，Memory Space 不变
- 冲突检测: 提交时比较读取版本与当前版本

## 8. 与 Object 的关系

- 每个 StateObject 拥有一个 MemorySpace。ModuleObject 是只读代码模板，不拥有 MemorySpace
- Memory Space 的生命周期与 Object 绑定
- Object 死亡时，其 Memory Space 被回收
- Object 冻结时，其 Memory Space 变为只读

## 9. 持久化与恢复

- Memory Space 的状态通过 WAL 持久化
- 崩溃恢复时，从 WAL 重建所有 Memory Space
- 未提交的 Transaction 写入不回放
- 已提交但 Effect 未确认的写入需要重试

## 10. 确定性

- 同样的初始 Memory Space 状态
- 加上同样的指令序列
- 产生同样的最终 Memory Space 状态
- 以及同样的版本号序列
- 这是 Veritas 确定性执行的基础

## 11. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| Memory Space | store.rs StateStore | 绑定到 Object |
| 版本管理 | store.rs StateEntry.version | 保持 |
| 冲突检测 | engine.rs detect_conflict | 移到 Machine 层 |
| 持久化 | wal.rs | 保持 |

## 12. 实现要求

1. Memory 不可被全局直接访问
2. 所有访问必须通过 (ObjectId, StateId) 寻址
3. 版本号单调递增
4. 写入必须在 Transaction 内
5. 确定性: 相同输入必得相同输出
