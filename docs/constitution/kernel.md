# Veritas Kernel Service Interface v0.2

## 1. Kernel 是什么

Kernel 不是 Object。Kernel 是 Machine 自身的内核执行模式。

Machine 有两种执行模式:
- 用户态 (User Mode): 执行 Object 的代码，受限访问
- 内核态 (Kernel Mode): 执行 Kernel Service，拥有完全特权

用户态通过 TRAP 指令进入内核态。内核服务执行完毕后通过
RETURN 指令返回用户态。

## 2. Kernel 调用机制

格式: TRAP <service_id>

参数通过寄存器和当前执行上下文传递。
返回值写入指定寄存器。

内核态拥有:
- 访问所有 Object 的 MemorySpace 的权限
- 管理 Capability 树的权限
- 分配 ObjectId 的权限
- 修改 ObjectRegistry 的权限

内核态不拥有:
- 独立的 MemorySpace (内核使用调用者的上下文)
- ObjectId (内核不是 Object，ObjectId=0 保留作为"调用者标识")
- 生命周期 (内核不被创建也不被销毁)

## 3. Kernel Service 列表

### 3.1 OBJECT_BIRTH
- 功能: 创建一个新 Object
- 参数: object_type (StateObject 或 ModuleObject)
- 返回: ObjectId
- 副作用: 新 Object 进入 BIRTH 状态，创建者获得 AdminCap
- 事务性: 是

### 3.2 OBJECT_DEATH
- 功能: 销毁一个 Object
- 参数: ObjectId
- 返回: 无
- 副作用: Object 进入 DEAD 状态，级联撤销所有 Capability，
  按 LinkType 语义清理所有 Link
- 事务性: 是

### 3.3 OBJECT_LINK
- 功能: 在两个 Object 之间建立 Link
- 参数: from_ObjectId, to_ObjectId, link_type
- 返回: 无
- 副作用: 建立带有 LinkType 的单向 Link
- 事务性: 是
- 校验: 自环 Link 拒绝，to 不存在拒绝，to 已死亡拒绝

### 3.4 OBJECT_FREEZE
- 功能: 冻结一个 Object
- 参数: ObjectId
- 返回: 无
- 副作用: Object 进入 FROZEN 状态，不可修改、不可建立新 Link
- 事务性: 是

### 3.5 CAPABILITY_GRANT
- 功能: 授予 Capability
- 参数: from_ObjectId, to_ObjectId, permissions
- 返回: CapabilityId
- 副作用: 在 Capability 树中创建新边
- 事务性: 是
- 校验: from 必须持有 AdminCap，to 必须存在且存活

### 3.6 CAPABILITY_REVOKE
- 功能: 撤销 Capability
- 参数: CapabilityId
- 返回: 无
- 副作用: 从 Capability 树中删除该边，级联撤销所有下游边
- 事务性: 是

### 3.7 MEMORY_ALLOC
- 功能: 为 StateObject 的 MemorySpace 分配新槽位
- 参数: ObjectId, size_hint
- 返回: StateId
- 事务性: 是

## 4. Kernel 与 Transaction 的关系

- 所有 Kernel Service 调用都在当前 Transaction 上下文中执行
- 内核服务的副作用在 Transaction 提交时生效
- Transaction 中止时，内核服务的副作用回滚
- 包括: Object 创建回滚、Capability 授予回滚、Link 建立回滚

## 5. Host Call 边界

某些能力 Veritas Machine 自身无法提供，需要外部环境支持。
这些通过 Host Call 接口暴露。

Host Call 与 Kernel Service 的区别:
- Kernel Service: Machine 内核态实现
- Host Call: Machine 之外的环境提供

Host Call 列表:

| 调用 | 功能 | 当前实现 |
|---|---|---|
| host_time | 获取当前时间戳 | Rust std::time |
| host_random | 获取随机字节 | Rust rand |
| host_write | 向外部输出 | stdout/stderr |
| host_read | 从外部输入 | stdin |
| host_spawn | 创建外部进程 | 未实现 |

调用方式: TRAP <host_call_id>
Host Call 执行时 Machine 暂停，等待外部环境返回结果。

## 6. Capability 模型

Capability 是 Kernel 管理的资源，不是 Object。

Capability:
- id: CapabilityId (64位，全局唯一)
- from: ObjectId (授权者)
- to: ObjectId (持有者)
- permissions: Read | Write | Execute | Admin 的组合
- parent: Option<CapabilityId> (上游 Capability，形成树状结构)

Capability 树:
- 每个 Object 创建时，创建者获得 AdminCap，作为树的根
- AdminCap 可以委托子 Capability 给其他 Object
- 子 Capability 可以进一步委托，形成树
- 撤销上游边时，下游级联撤销
- Object 死亡时，所有指向它的 Capability 全部撤销

## 7. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| Kernel Mode | 不存在 | 新增 Machine 执行模式 |
| TRAP 机制 | machine.rs trap_frame | 保持并扩展 |
| Kernel Service | engine.rs pub fn 方法 | 转为 TRAP 处理 |
| Capability 树 | capability.rs | 改为 Kernel Resource |
| Host Call | 散落各处 | 统一接口 |

## 8. 实现要求

1. Kernel 不是 Object，是 Machine 内核态
2. 所有内核服务通过 TRAP 调用
3. 内核服务必须事务性
4. Capability 是 Kernel Resource，不是 Object
5. Host Call 统一收口
