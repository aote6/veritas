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

状态机: （不存在）→ Birth/Commit → Alive → Frozen → Dead

- Alive: 正常状态，可读写、可建立 Link（代码/数据模型统一使用 Alive）
- Frozen: 只读状态，不可修改、不可建立新 Link
- Dead: 不可访问（终态，不可逆）。
  - 所有指向该 Object 的 Capability 自动失效（validity depends on
    target object liveness；使用时校验 resource 必须 Alive，不在
    Death 路径主动遍历清扫 Capability 图）
  - 所有 Link 按 LinkType 语义处理（见 link.md）：
    - OWNS：owned 对象级联进入 DEAD
    - DEPENDS_ON：向 dependent 发出 DependencyInvalidated
    - REFERENCES：删除边，无通知、无级联
  - ObjectId 永久保留，不可重用

状态转换:

| 从 | 到 | 触发指令 |
|---|---|---|
| 不存在 | Birth/Commit | OBJECT_BIRTH |
| Alive | Frozen | OBJECT_FREEZE |
| Frozen | Dead | OBJECT_DEATH |
| Alive | Dead | OBJECT_DEATH |

## 5. Object 的组成

以下为 Object 的逻辑组成（机器可见概念），不是 ObjectRecord 的物理字段。
物理字段定义见运行时数据模型标准（id, object_type, state, body）。

- id: ObjectId
- type: ObjectType (StateObject | ModuleObject)
- state: Alive | Frozen | Dead
- body: State | Module { code_section, import_section, export_section, verification_rule? }
- memory_space: 该 Object 拥有的 MemorySpace (StateObject 有，ModuleObject 无)
- capability_space: 该 Object 作为 holder 持有的 Capability
  （由 Capability 图按 holder 索引，非 Object 内嵌列表）
- incoming_links / outgoing_links: 由 topology 按端点投影，非 ObjectRecord 字段
- history: 创建与修改的 Transaction 记录（ExecutionHistory）

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

ModuleObject 死亡时，所有基于它的 ModuleInstance 收到 TRAP。（注：未实现，缺口）

## 7. Object 之间的 Link

Link 不是裸边。Link 有明确的语义类型:

| LinkType | 含义 | to 死亡 | from 死亡 | to 冻结 |
|---|---|---|---|---|
| DEPENDS_ON | from 依赖 to | to 死亡 → from 收到 DependencyInvalidated | Link 清理 | from 不可修改 to |
| OWNS | from 拥有 to | to 级联死亡 | from 死亡 → to 级联死亡 | from 不可修改 to |
| REFERENCES | from 引用 to | Link 断开 | Link 清理 | 无影响 |

Link 操作由 OBJECT_LINK 指令触发，需要指定 LinkType。

## 8. Object 与 Transaction 的关系

- OBJECT_BIRTH 必须在 Transaction 内
- Transaction 提交后 Object 进入 Alive 状态
- Transaction 中止时 Object 创建回滚，不留痕迹
- 修改 MemorySpace 必须在 Transaction 内
- OBJECT_DEATH 必须在 Transaction 内

## 9. Object 与 Capability 的关系

- Capability 是 Kernel 管理的资源，不是 Object
- 每个 Object 持有 capability_space: 一组 Capability 的集合
- Object 创建时，创建者自动获得该 Object 的 AdminCap
- 通过 CAPABILITY_GRANT 将 Capability 授予其他 Object
- 通过 CAPABILITY_REVOKE 撤销已授予的 Capability
- Object 死亡后，所有以该 Object 为 resource 的 Capability 自动失效
  （lazy validation：授权入口检查 resource.is_alive()）
- Object 作为 holder 死亡时，其持有的 Capability 不再可用
  （revoke_holder 与/或使用时 holder 存活检查）
- 不要求在 Death 提交路径上物理扫描并删除全部 Capability 记录

## 10. 当前实现映射

| 规范定义 | 当前代码 | 状态 |
|------|----------|------|
| ObjectId | types.rs | done |
| ObjectType / ObjectRecord | types.rs | done (P30) |
| ObjectRegistry | engine.rs object_registry | done |
| Birth / Freeze / Death | engine.rs | done |
| OWNS cascade | expand_owns_death_closure | done (P8.1) |
| DEPENDS_ON invalidated | DependencyInvalidated | done (P8.2) |
| Capability lazy resource liveness | verify_capability | done (P8.3) |
| LinkType | engine.rs topology | done |
| MemorySpace | state_memory.rs | done |
| ModuleObject / Instance | module.rs | partial |

## 11. 实现要求

任何 Veritas 实现必须满足:

1. ObjectId 全局唯一，不可重用
2. Object 生命周期状态机不可绕过
3. Capability 不是 Object，是 Kernel Resource
4. ModuleObject 与 ModuleInstance 分离
5. Link 必须携带 LinkType，不可作为裸边存在
6. Capability 检查发生在 Machine 执行层，不可跳过
7. Object 创建与删除必须事务性
