# Veritas Object Specification v0.1

## 1. Object 是什么

Object 是 Veritas 世界中的一等公民。一切状态、代码、权限在 Veritas 中都表示为 Object。

Object 不是 Rust struct。不是 HashMap 里的 entry。Object 是 Veritas Machine 能够识别和操作的机器原语。

## 2. Object 的身份

ObjectId: 64位无符号整数，全局唯一

- ObjectId 由 Machine 在创建 Object 时分配
- ObjectId 不可重用，即使 Object 死亡，其 ID 永久保留
- ObjectId 与创建该 Object 的 Transaction 绑定，确定性生成

## 3. Object 的类型

| 类型 | 含义 |
|---|---|
| StateObject | 可读写的状态容器，拥有自己的 Memory Space |
| ModuleObject | 可执行的代码，携带权限声明与验证规则 |
| CapabilityObject | 访问其他 Object 的凭证 |

ObjectType 是 ISA 级别的概念，存储在 Object 的元数据中。
Machine 在执行指令时根据类型决定允许的操作。

## 4. Object 的生命周期

状态机: BIRTH -> ACTIVE -> FROZEN -> DEAD

- BIRTH: Object 被创建，获得初始 Capability，进入 ObjectRegistry
- ACTIVE: 正常状态，可读写、可授权、可建立 Link
- FROZEN: 只读状态，不可修改、不可授予新 Capability
- DEAD: 不可访问，从 ObjectRegistry 移除，所有指向它的 Capability
  级联撤销，所有 Link 清理

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
- memory_space: 该 Object 拥有的内存区域
- capability_space: 该 Object 持有的 Capability 列表
- incoming_links: 指向该 Object 的其他 Object ID 列表
- outgoing_links: 该 Object 指向的其他 Object ID 列表
- history: 创建与修改的 Transaction 记录
- verification_rule: 仅 ModuleObject 携带

## 6. Object 的 Memory Space

每个 StateObject 和 ModuleObject 拥有自己的 Memory Space。
Memory Space 是 Object 的一部分，不是全局 HashMap。

MemorySpace:
- object_id: 所属 Object
- slots: 状态槽位列表
- version: 当前版本号

访问 Memory Space 必须通过 Capability 验证。
验证发生在 Machine 执行 STORE/LOAD 指令时，不在 engine API 层。

## 7. Object 的 Capability 模型

- 每个 Object 创建时，创建者自动获得该 Object 的 AdminCap
- AdminCap 可以委托子 Capability 给其他 Object
- 子 Capability 可以进一步委托，形成树状结构
- 撤销上游 Capability 时，下游级联撤销
- Object 死亡时，所有指向它的 Capability 全部撤销

## 8. Object 之间的 Link

- Object 可以 Link 到其他 Object
- Link 是单向的
- 自环 Link 被拒绝
- Object 死亡时，所有进出 Link 清理
- Link 操作由 OBJECT_LINK 指令触发

## 9. Object 与 Transaction 的关系

- OBJECT_BIRTH 必须在 Transaction 内
- Transaction 提交后 Object 进入 ACTIVE 状态
- Transaction 中止时 Object 创建回滚，不留痕迹
- 修改 Memory Space 必须在 Transaction 内
- OBJECT_DEATH 必须在 Transaction 内

## 10. 当前实现映射

| 规范定义 | 当前代码位置 | 未来方向 |
|---|---|---|
| ObjectId | types.rs | 保持 |
| ObjectType | 散落各处 | 统一定义在 ISA 规范 |
| ObjectRegistry | engine.rs HashMap | 独立为 Machine 组件 |
| BIRTH/DEATH | engine.rs 方法 | 转为 ISA 指令 |
| MemorySpace | state_memory.rs | 与 Object 绑定 |
| Capability | capability.rs | 保持，成为 Object 属性 |
| Link/Topology | engine.rs topology | 与 Object 绑定 |

## 11. 实现要求

任何 Veritas 实现必须满足:

1. ObjectId 全局唯一，不可重用
2. Object 生命周期状态机不可绕过
3. Memory Space 绑定到 Object，不可全局共享
4. Capability 检查发生在 Machine 执行层，不可跳过
5. Object 创建与删除必须事务性
