# TRAP 化现状审计

**日期**: 2026-08-20
**状态**: 审计完成，未改代码
**目的**: 厘清 Machine 到 TRAP 到 Kernel Service 到 Engine 的真实调用边界，为最小迁移闭环提供依据


## 1. 核心结论

Veritas 的 TRAP 化已经部分存在，但存在双轨制：

1. Instruction::Trap 已经存在，并已分发到 Kernel::handle
2. 旧式专用指令仍然直接构造 KernelCall 调用 Kernel::handle
3. KernelCall::decode 已经定义了 TRAP ABI，但没有被所有路径使用
4. WorldService 是 Kernel Service 的高层调用者，绕过 Machine 直接调用 Kernel::handle 和 Engine 内部方法

判断: TRAP 化的基础设施已经就位，缺的是统一收口和旧式指令的退役。


## 2. 入口分类总表

### 2.1 Kernel Service（应走 TRAP，已部分实现）

| 入口 | 当前调用者 | 是否已走 TRAP | 参数 ABI | 返回 ABI | 事务边界 | 优先级 |
|---|---|---|---|---|---|---|
| KernelCall::ObjectBirth | Machine(旧式指令+TRAP)、WorldService | 部分 | object_type | ObjectId | 需外部 begin/commit | P0 |
| KernelCall::ObjectDeath | Machine(旧式指令+TRAP)、WorldService | 部分 | object_id | Success | 需外部 begin/commit | P0 |
| KernelCall::ObjectLink | Machine(旧式指令+TRAP)、WorldService | 部分 | from,to,link_type | Success | 需外部 begin/commit | P0 |
| KernelCall::ObjectUnlink | Machine(旧式指令+TRAP)、WorldService | 部分 | from,to | Success | 需外部 begin/commit | P0 |
| KernelCall::ObjectFreeze | Machine(旧式指令+TRAP)、WorldService | 部分 | object_id | Success | 需外部 begin/commit | P0 |
| KernelCall::CapabilityGrant | Machine(旧式指令+TRAP)、WorldService | 部分 | grantor,grantee,type,resource | CapabilityId | 需外部 begin/commit | P0 |
| KernelCall::CapabilityRevoke | Machine(TRAP)、WorldService | 部分 | cap_id,holder,cascade | Success | 需外部 begin/commit | P0 |
| KernelCall::CapabilityDelegate | Machine(TRAP)、WorldService | 部分 | cap_id,from,to,cascade | Success | 需外部 begin/commit | P0 |
| KernelCall::MemoryAlloc | Machine(TRAP) | 是 | object_id,size | Success | 需外部 begin/commit | P0 |
| KernelCall::Commit | Machine(旧式指令+TRAP)、WorldService | 部分 | 无 | Success | 结束事务 | P0 |
| KernelCall::Abort | Machine(旧式指令+TRAP)、WorldService | 部分 | reason | Success | 结束事务 | P0 |
| KernelCall::Effect | Machine(旧式指令+TRAP) | 部分 | payload | EffectKey | 需外部 begin/commit | P0 |
| KernelCall::Savepoint | Machine(旧式指令+TRAP) | 部分 | name | Success | 事务内 | P0 |
| KernelCall::RollbackTo | Machine(旧式指令+TRAP) | 部分 | name | Success | 事务内 | P0 |


### 2.2 Engine 内部 API（不应走 TRAP，是 Kernel 实现细节）

| 入口 | 当前调用者 | 说明 |
|---|---|---|
| Engine::begin | Kernel(内部)、WorldService | 事务分配，不是机器能力 |
| Engine::begin_in_object | Kernel(内部)、WorldService | 事务上下文切换 |
| Engine::commit | Kernel(内部)、WorldService | 事务提交核心 |
| Engine::read/write/effect | Kernel(内部) | 状态读写 |
| Engine::init_state_in_tx | Kernel(内部) | 状态初始化 |
| Engine::savepoint/rollback_to | Kernel(内部) | 事务嵌套控制 |
| Engine::capability_* | Kernel(内部) | Capability 操作 |
| Engine::object_* | Kernel(内部) | 对象生命周期 |
| Engine::apply | Kernel(replay)、WAL | Delta 应用，不是机器能力 |
| Engine::build_delta | Engine(commit 内部) | 提交时构造 |
| Engine::detect_conflict | Engine(commit 内部) | OCC 冲突检测 |
| Engine::create_checkpoint/restore_checkpoint | Kernel(内部) | 世界状态快照 |


### 2.3 预留组件（与 TRAP 无关）

| 入口 | 状态 | 说明 |
|---|---|---|
| LockManager::acquire | 预留 | 未接入 commit 路径，Wound-Wait 锁管理器 |
| HostCall | 预留 | 外部能力，独立 service_id 范围，非 Kernel Service |
| ModuleObject/ModuleInstance | 预留 | 模块化对象，未接入主链 |
| Extension trait | 预留 | 扩展点，未使用 |


## 3. 双轨制详解

### 3.1 旧式专用指令路径

这些指令在 Machine::step 中被硬编码处理，直接构造 KernelCall 并调用 kernel.handle：

| Instruction 变体 | 位置(machine.rs) | 对应的 KernelCall |
|---|---|---|
| Instruction::Commit | 357 | KernelCall::Commit |
| Instruction::Abort | 366 | KernelCall::Abort |
| Instruction::CapabilityGrant | 375 | KernelCall::CapabilityGrant |
| Instruction::Effect | 400 | KernelCall::Effect |
| Instruction::Savepoint | 410 | KernelCall::Savepoint |
| Instruction::RollbackTo | 420 | KernelCall::RollbackTo |
| Instruction::ObjectBirth | 620 | KernelCall::ObjectBirth |
| Instruction::ObjectDeath | 658 | KernelCall::ObjectDeath |
| Instruction::ObjectFreeze | 669 | KernelCall::ObjectFreeze |
| Instruction::ObjectLink | 680 | KernelCall::ObjectLink |
| Instruction::ObjectUnlink | 699 | KernelCall::ObjectUnlink |


### 3.2 新式 TRAP 路径（已存在）

Instruction::Trap 在 Machine::step 535-570 行：

1. 读取 r0/r1/r2
2. 调用 KernelCall::decode(service_id, r0, r1, r2)
3. 分发到 kernel.handle
4. 将 TrapResult 写回 r0

TRAP 基础分发路径已经实现，并存在一套当前可用的基础 ABI；但 ABI 尚未覆盖全部 KernelCall。


### 3.3 双轨制的根源

KernelCall::decode 已经定义了完整的 service_id 映射，但 Instruction 枚举仍然保留了所有旧式指令变体。这导致：

- 同一操作有两种编码方式
- Machine 需要同时维护两套分发逻辑
- 测试和 WorldService 可以绕过 Machine 直接调用 Kernel


## 4. TRAP ABI 现状

### 4.1 已定义的 service_id

| service_id | KernelCall | 参数来源 | 返回 |
|---|---|---|---|
| 0 | ObjectBirth | r0=object_type | ObjectId 到 r0 |
| 1 | ObjectDeath | r0=object_id | Success |
| 2 | ObjectLink | r0=from, r1=to | Success |
| 3 | ObjectUnlink | r0=from, r1=to | Success |
| 4 | ObjectFreeze | r0=object_id | Success |
| 5 | Commit | 无 | Success |
| 6 | Effect | r0=payload 未实现 | EffectKey |
| 7 | Savepoint | r0=name 未实现 | Success |
| 8 | RollbackTo | r0=name 未实现 | Success |

注意: CapabilityGrant、CapabilityRevoke、CapabilityDelegate、MemoryAlloc 虽然出现在 KernelCall 枚举中，但没有出现在 decode 的 service_id 映射里。


### 4.2 缺失的 ABI

1. CapabilityGrant 的参数远超 r0/r1/r2 能承载
2. Effect payload 是 Vec<u8>，无法通过 r0/r1/r2 传递
3. Savepoint/RollbackTo name 是 String，无法通过 r0/r1/r2 传递
4. 返回值的错误处理: TrapResult 没有错误变体


## 5. 调用关系图（现状）

Machine (machine.rs)
  旧式指令: Commit/Abort/ObjectBirth 等直接构造 KernelCall 调 Kernel::handle
  Trap 走 KernelCall::decode 调 Kernel::handle
  HostCall 空操作

WorldService (world_api.rs)
  直接调用 Kernel::handle 和 Kernel::begin/commit
  直接调用 Engine 内部方法
  维护 session 状态和 identity

Kernel (kernel.rs)
  handle 转 Engine 内部方法
  begin/begin_in_object 转 Engine::begin
  commit 转 Engine::commit
  其他 pub(crate) 内部方法转 Engine 对应方法


## 6. 最小迁移闭环建议（第一批）

不在此轮执行，仅记录设计意图。

候选: 建立 TRAP 与旧式入口的等价性验证，不删除旧式指令

第一批范围（仅简单 ABI）:
- ObjectBirth (service_id 0)
- ObjectDeath (service_id 1)
- ObjectLink (service_id 2)
- ObjectUnlink (service_id 3)
- ObjectFreeze (service_id 4)
- Commit (service_id 5)

目标:
1. 证明 Machine 的 TRAP 路径与旧式指令路径产生相同的 KernelCall 语义
2. 证明两条路径产生相同的事务结果和 World State commitment
3. 锁住等价性测试后，再谈旧式指令退役

暂不处理:
- CapabilityGrant / CapabilityRevoke / CapabilityDelegate（参数超出 r0/r1/r2）
- Effect / Savepoint / RollbackTo（需要内存 ABI）
- 旧式指令删除


## 7. 本轮不做的

1. 不修改任何代码
2. 不设计复杂参数 ABI（CapabilityGrant、Effect、Savepoint 等）
3. 不强制 WorldService 经过 Machine/TRAP
4. 不删除任何旧式指令
5. 不改变 HostCall 状态
6. 不开始第一批 TRAP 等价性测试（下一轮再做）


## 8. 审计后需要回答的问题

1. 旧式指令是否全部有对应的 TRAP service_id？否，CapabilityGrant/Revoke/Delegate 缺失；但简单 KernelCall（ObjectBirth/Death/Link/Unlink/Freeze/Commit）已有
2. 复杂参数如何通过 TRAP ABI 传递？未定义，需要内存传递或状态寄存器扩展
3. WorldService 是系统软件高层 API，不需要经过 Machine/TRAP；但其调用的 Kernel Service 语义必须与 TRAP 路径一致
4. 第一批迁移后，旧式指令是否可以立即删除？否。先做等价性验证，锁住测试后再逐个退役


## 9. 文件索引

- src/kernel.rs — KernelCall 定义、decode、handle
- src/instruction.rs — Instruction 枚举，含 Trap 和旧式指令
- src/instruction_codec.rs — 指令编码
- src/machine.rs — Machine::step 双轨制分发
- src/world_api.rs — WorldService 高层入口
- src/engine.rs — Engine 内部实现


审计完成日期: 2026-08-20
审计者: 基于代码 grep 和 sed 分析
代码状态: 未修改
