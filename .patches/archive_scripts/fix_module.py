content = '''# Veritas Module Specification v0.2

## 1. Module 是什么

Module 是 Veritas 中的可执行代码单元。它是一个 ModuleObject。

ModuleObject 是只读的代码模板，加载后默认 FROZEN。
要执行 Module，必须创建 ModuleInstance。

## 2. ModuleObject 的结构

ModuleObject 是 ObjectType = ModuleObject 的 Object:

- id: ObjectId
- type: ModuleObject
- code_section: 指令序列 (不可变)
- import_section: 依赖的其他 Module 的 ObjectId 列表
- export_section: 对外暴露的入口点列表 (名称 -> 指令偏移)
- verification_rule: 执行前必须满足的验证规则
- capability_space: 该 ModuleObject 持有的 Capability
- incoming_links: 指向该 ModuleObject 的 Link
- outgoing_links: 该 ModuleObject 指向其他 Object 的 Link
- history: 创建与修改的 Transaction 记录

注意: ModuleObject 没有 memory_space。代码不可变，不需要可变存储。

## 3. ModuleInstance 的结构

ModuleInstance 是执行 ModuleObject 的运行时实体。
它是一个 StateObject:

- id: ObjectId
- type: StateObject
- module: 指向 ModuleObject 的 ObjectId
- memory_space: 该实例的私有 MemorySpace
- capability_space: 该实例持有的 Capability
- pc: 程序计数器 (当前执行位置)
- incoming_links: 指向该实例的 Link
- outgoing_links: 该实例指向其他 Object 的 Link
- history: 创建与修改的 Transaction 记录

## 4. 创建和执行流程

加载并执行一个 Module:

1. ModuleObject 已存在 (之前通过 OBJECT_BIRTH 创建，处于 FROZEN 状态)
2. 创建 ModuleInstance: OBJECT_BIRTH type=StateObject
3. 设置 instance.module = ModuleObject 的 ObjectId
4. 将 ModuleObject 的 code_section 映射到 instance 的执行空间
5. 根据 ModuleObject 的 import_section 解析依赖:
   - 对每个依赖的 Module，通过 Capability 获取其 ModuleObject
   - 为每个依赖创建对应的 ModuleInstance (如果需要)
6. 根据 ModuleObject 的 export_section 设置入口点
7. 通过 CAPABILITY_GRANT 授予 instance 所需的 Capability
8. instance 进入 ACTIVE 状态
9. Machine 设置 PC = 入口点偏移，开始执行
10. 执行完毕后，可以选择保留 instance 或销毁

## 5. ModuleInstance 的生命周期

ModuleInstance 和 ModuleObject 的生命周期是独立的:

- 删除 ModuleInstance: 只回收该实例的 MemorySpace 和 Capability
- 删除 ModuleObject: 所有基于它的 ModuleInstance 收到 TRAP，
  可以选择保存状态后自行销毁
- 一个 ModuleObject 可以有零个、一个或多个 ModuleInstance
- ModuleInstance 可以存活超过创建它的 ModuleInstance

## 6. Module 之间的调用

- ModuleInstance A 调用 ModuleInstance B 的函数:
  1. A 持有 B 的 ModuleObject 的 ExecuteCap
  2. CALL 指令携带 B 的 ObjectId 和函数名
  3. Machine 查找 B 的 export_section，获取入口偏移
  4. Machine 保存 A 的 PC，设置 PC = B 的入口偏移
  5. B 执行完毕后通过 RETURN 指令返回 A

- 如果 B 还没有 ModuleInstance:
  1. Machine 自动创建 B 的 ModuleInstance
  2. 或者在调用前手动创建并授予 Capability

## 7. Module 的验证规则

ModuleObject 携带验证规则。验证规则在创建 ModuleInstance 时
和执行前由 Machine 检查。

验证规则类型:
- allow_objects: 允许访问的 ObjectId 白名单
- allow_instructions: 允许的指令类型限制
- allow_capability_ops: 允许的 Capability 操作限制
- max_instances: 最大 Instance 数量限制
- max_executions: 最大执行次数限制

## 8. 当前格式

当前格式: .vmod 二进制格式

未来应定义 ModuleObject 和 ModuleInstance 的序列化格式，
使其可以直接存储在 MemorySpace 中，支持持久化和迁移。

## 9. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| ModuleObject | module.rs ModuleImage | 改为只读代码模板 |
| ModuleInstance | 不存在 | 新增，作为 StateObject 子类型 |
| .vmod 格式 | module.rs | 保持或扩展 |
| 加载器 | module.rs ModuleLoader | 分离模板加载和实例创建 |
| 执行 | machine.rs boot/step | 支持 CALL/RETURN 跨 Module |

## 10. 实现要求

1. ModuleObject 是只读代码模板，ModuleInstance 是执行实例
2. ModuleObject 没有 MemorySpace，ModuleInstance 有
3. 跨 Module 调用通过 CALL + Capability
4. 验证规则是 ModuleObject 的属性，Machine 自动检查
5. ModuleObject 死亡不影响已有 ModuleInstance
'''

with open('docs/constitution/module.md', 'w') as f:
    f.write(content)
print('Done: docs/constitution/module.md updated to v0.2')
