# Deterministic World（Stage 1 设计文档）

版本：v1.0
日期：2026-08-04
状态：已冻结

## 1. World State 判别标准

一个数据属于 World State，必须同时满足两条：

### 标准一：影响未来机器行为（Future Behavior）
同样的世界状态，如果这个值不同，下一条合法指令可能产生不同结果。

### 标准二：不可由其他 World State 纯函数推导
如果可以确定性重新计算，它就不是独立机器状态，不进入 World State。

---

## 2. World State 分类表

| 项目 | 影响未来行为 | 可纯函数推导 | 属于 World State | 说明 |
|------|:-----------:|:----------:|:--------------:|------|
| **StateStore** | 是 | 否 | ✅ | 唯一可变状态值 |
| **ObjectRegistry** | 是 | 否 | ✅ | Object 生命周期与类型 |
| **Topology** | 是 | 否 | ✅ | Object 间 Link 关系 |
| **CapabilityGraph** | 是 | 否 | ✅ | 跨 Object 授权 |
| **ScopeRegistry** | 是 | 否 | ✅ | Scope 成员绑定 |
| **global_version** | 是 | 否 | ✅ | 决定 snapshot_version 与冲突检测 |
| **object_id_counter** | 是 | 否 | ✅ | 决定下一次 ObjectId 分配 |
| **grant_sequence** | 是 | 否 | ✅ | 决定下一次 CapabilityId 分配 |
| **WAL Position** | 视设计而定 | 视设计而定 | 待定 | 是否需要断点续传 |
| TransactionContext | 否 | — | ❌ | 恢复时不存在活跃事务 |
| Registers | 否 | — | ❌ | 同上，仅 Transaction 内有意义 |
| Call Stack | 否 | — | ❌ | Checkpoint 在 commit 边界，call stack 永远为空 |
| Effect Queue | 取决于 Effect 模型 | 待定 | 待定 | 若 Effect 不在承诺域则可排除 |
| Receipt | 否 | 可生成 | ❌ | 证明材料，不是机器状态 |

---

## 3. 当前 WorldSnapshot 与分类表的差距

当前 `WorldSnapshot` 结构（types.rs:745）包含：

| 字段 | 是否属于 World State | 
|------|:------------------:|
| commitment_hash | ❌ 可重新计算 |
| tx_id | ❌ 可推导 |
| state_entries | ✅ |
| capability_records | ✅ |
| objects | ✅ |
| links | ✅ |
| scopes | ✅ |

缺失的 World State：
- global_version
- object_id_counter
- grant_sequence
- 每个 StateEntry 的真实 version（当前硬编码为 1）

---

## 4. 待回答问题

1. Replay 的目标：第三方验证世界状态，还是恢复执行？
2. Receipt 要证明什么？
3. Effect 是否进入承诺域？
4. WAL Position 是否需要作为 World State？
5. root_hash 的宽度：u64 是否足够？


## 6. Receipt 设计

### 当前结构

| 字段 | 含义 |
|------|------|
| tx_id | 事务ID |
| before_root | 事务前 World Commitment |
| delta | TransactionDelta（births/deaths/writes/links/caps） |
| after_root | 事务后 World Commitment |

### 目标C下的Receipt要求

验证者拥有Receipt + WAL（或Receipt链），即可：
1. 从已知WorldSnapshot（或before_root对应世界）出发
2. apply(delta) 得到新世界
3. 计算after_root并与Receipt中的after_root比对
4. 一致即证明：此delta在此世界前后状态是合法的

Receipt链 = 初始WorldSnapshot + Receipt序列，即可验证整个世界演化。

## 7. Effect 判定

标准一（影响未来行为）：Effect是外部输出，不改变后续合法指令结果 → 否
标准二（可纯函数推导）：不能从World State推导，但不影响World State → 不适用

结论：Effect不属于World State，不进入root_hash。
Effect的至少一次语义属于Effect自身设计问题，不影响Deterministic World边界。

## 8. WAL Position 判定

标准一：影响恢复后的世界状态？否。从WAL头重放结果相同。
标准二：可从WAL+初始世界推导恢复终点？是。

结论：WAL Position是恢复优化元数据，不属于World State。

## 9. root_hash 宽度

当前：u64（FNV-1a），碰撞概率在2^32条事务后约50%%，不满足第三方验证。
要求：Commitment必须是密码学安全的（SHA-256或BLAKE3 256-bit）。
架构规定算法强度，具体算法可替换，不进入Constitution。

## 10. Stage 1 结论

### World State 完整定义

World State = StateStore + ObjectRegistry + Topology + CapabilityGraph + ScopeRegistry + global_version + object_id_counter + grant_sequence

所有 StateEntry 保留真实 version（非硬编码1）。

### 不进入 World State
TransactionContext, Registers, Call Stack, Effect, Receipt, WAL Position

## 11. 最高原则：Machine State vs Program State

World State 是软件计算机在某一机器周期结束后的全部硬件状态。

Checkpoint、Replay、Receipt、Recovery 都必须以恢复或验证这一硬件状态为目标，
而不是恢复某个运行过程。

Veritas 保存的是机器状态（Machine State），不是程序状态（Program State）。

判别标准：
- 机器周期结束后仍存在的硬件状态 → World State
- 机器周期内存在的临时执行状态 → 不属于 World State

## 12. Stage 2 实现任务

### 主线（顺序执行）

1. WorldSnapshot 扩展为八组件（五组件 + global_version + object_id_counter + grant_sequence）
2. StateEntry 保留真实 version（不再硬编码 1）
3. Object Death 清理 StateStore（闭合 Machine State）
4. Checkpoint 保存/恢复完整 Machine State
5. Recovery 恢复完整 Machine State（计数器续接 + 版本真实）
6. Replay 统一走 TransactionDelta → apply()（禁止第二套路径）
7. root_hash 升级为 SHA-256 World Commitment

### 并行修复（不阻塞主线，Stage 2 结束前完成）

A. verify_capability 覆盖所有受保护操作（读/link/death/freeze/CALL）
B. Kernel API 收敛为 handle(KernelCall) + 受控 query

### 执行顺序的逻辑

Machine State 的定义 → 完整性 → 保存 → 恢复 → 验证
先闭合 World State 自身，再做保护机制（Capability）和入口控制（Kernel）。

## 13. 已知技术债

### Kernel API 未完全收敛

begin/read/write/commit 等 mutation API 当前仍为 pub，
原因是测试与内部组件仍直接调用。

最终目标：所有世界状态修改必须通过 KernelCall → Kernel::handle()。
迁移条件：Kernel ABI 固化后统一收敛。

### empty() 持有占位 WAL 文件

Replay 不应打开 WAL 文件，需要 WalSink trait 或 Option&lt;WalWriter&gt; 重构。
