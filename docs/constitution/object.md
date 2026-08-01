# Veritas Object Specification v0.2

## 1. Object 是什么

Object 是 Veritas 世界中的一等公民。一切状态和代码在 Veritas 中
都表示为 Object。

Object 不是 Rust struct。不是 HashMap 里的 entry。Object 是
Veritas Machine 能够识别和操作的机器原语。

## 2. Object 的身份

ObjectId: 64位无符号整数，全局唯一

- ObjectId 由 Machine 在创建 Object 时分配
- ObjectId 不可重用，即使 Object 死亡，其 ID 永久保留
- ObjectId 与创建该 Object 的 Transaction 绑定，确定性生成

## 3. Object 的类型

ObjectType 只有两种:

| 类型 | 含义 |
|---|---|
| StateObject | 拥有 MemorySpace，存储可变状态 |
| ModuleObject | 拥有代码段，不可变，加载后默认 FROZEN |

注意: Capability 不是 Object。Capability 是 Kernel 管理的资源，
详见 Kernel Service Interface。

## 4. Object 的生命周期

状态机: BIRTH -> ACTIVE -> FROZEN -> DEAD

- BIRTH: Object 被创建，获得初始 Capability，进入 ObjectRegistry
- ACTIVE: 正常状态，可读写、可建立 Link
- FROZEN: 只读状态，不可修改、不可建立新 Link
- DEAD: 不可访问，从 ObjectRegistry 移除，所有指向它的 Capability
  级联撤销，所有 Link 按 LinkType 语义清理

状态转换:

| 从 | 到 | 触发指令 |
|---|---|---|
| 不存在 | BIRTH | OBJECT_BIRTH |
| BIRTH | ACTIVE | 创建 Transaction 提交 |
| ACTIVE | FROZEN | OBJECT_FREEZE |
| FROZEN | DEAD | OBJECT_DEATH |
| ACTIVE | DEAD | OBJECT_DEATH |

## 5. Object 的组成

每个 Object 包含:

- id: ObjectId
- type: ObjectType
- memory_space: 该 Object 拥有的 MemorySpace (StateObject 有，
  ModuleObject 无)
- code_section: 指令序列 (仅 ModuleObject 有)
- import_section: 依赖的其他 Module (仅 ModuleObject 有)
- export_section: 对外暴露的入口点 (仅 ModuleObject 有)
- verification_rule: 执行前必须满足的验证规则 (仅 ModuleObject 有)
- capability_space: 该 Object 持有的 Capability 列表 (Kernel Resource，
  不是 Object)
- incoming_links: 指向该 Object 的 Link 列表
- outgoing_links: 该 Object 指向其他 Object 的 Link 列表
- history: 创建与修改的 Transaction 记录

## 6. ModuleObject 与 ModuleInstance

ModuleObject 是只读的代码模板，加载后默认 FROZEN。

要执行 ModuleObject，需要创建 ModuleInstance:

ModuleInstance:
- 是一个 StateObject
- module: 指向 ModuleObject 的 ObjectId
- memory_space: 该实例的私有 MemorySpace
- capability_space: 该实例持有的 Capability
- pc: 程序计数器

多个 ModuleInstance 可以共享同一个 ModuleObject。
ModuleObject 死亡时，所有基于它的 ModuleInstance 收到 TRAP。

## 7. Object 之间的 Link

Link 不是裸边。Link 有明确的语义类型:

| LinkType | 含义 | to 死亡 | from 死亡 | to 冻结 |
|---|---|---|---|---|
| DEPENDS_ON | from 依赖 to | from 收到 TRAP | Link 清理 | from 不可修改 to |
| OWNS | from 拥有 to | from 收到 TRAP，to 不可单独存活 | to 级联死亡 | from 不可修改 to |
| REFERENCES | from 引用 to | Link 断开 | Link 清理 | 无影响 |

Link 操作由 OBJECT_LINK 指令触发，需要指定 LinkType。

## 8. Object 与 Transaction 的关系

- OBJECT_BIRTH 必须在 Transaction 内
- Transaction 提交后 Object 进入 ACTIVE 状态
- Transaction 中止时 Object 创建回滚，不留痕迹
- 修改 MemorySpace 必须在 Transaction 内
- OBJECT_DEATH 必须在 Transaction 内

## 9. Object 与 Capability 的关系

- Capability 是 Kernel 管理的资源，不是 Object
- 每个 Object 持有 capability_space: 一组 Capability 的集合
- Object 创建时，创建者自动获得该 Object 的 AdminCap
- 通过 CAPABILITY_GRANT 将 Capability 授予其他 Object
- 通过 CAPABILITY_REVOKE 撤销已授予的 Capability
- Object 死亡时，所有指向它的 Capability 级联撤销

## 10. 当前实现映射

| 规范定义 | 当前代码位置 | 未来方向 |
|---|---|---|
| ObjectId | types.rs | 保持 |
| ObjectType | 散落各处 | 统一定义，删除 CapabilityObject |
| ObjectRegistry | engine.rs HashMap | 独立为 Machine 组件 |
| BIRTH/DEATH | engine.rs 方法 | 转为 ISA 指令 |
| MemorySpace | state_memory.rs | 与 Object 绑定 |
| ModuleObject/Instance | module.rs | 分离代码模板和执行实例 |
| Link/LinkType | engine.rs topology | 增加语义类型 |
| Capability | capability.rs | 改为 Kernel Resource，不属于 Object |

## 11. 实现要求

任何 Veritas 实现必须满足:

1. ObjectId 全局唯一，不可重用
2. Object 生命周期状态机不可绕过
3. Capability 不是 Object，是 Kernel Resource
4. ModuleObject 与 ModuleInstance 分离
5. Link 必须携带 LinkType，不可作为裸边存在
6. Capability 检查发生在 Machine 执行层，不可跳过
7. Object 创建与删除必须事务性
