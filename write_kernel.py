content = '''# Veritas Kernel Service Interface v0.1

## 1. 内核服务是什么

内核服务是 Veritas Machine 提供的一组基础能力。这些能力不能由
用户态 Object 自行完成，必须通过 Trap 进入内核执行。

## 2. 内核调用机制

用户代码通过 TRAP 指令进入内核。TRAP 指令携带一个操作码，指示
需要哪种内核服务。

格式: TRAP <service_id>

参数通过寄存器和当前执行上下文传递。

## 3. 内核服务列表

### 3.1 OBJECT_BIRTH
- 功能: 创建一个新 Object
- 参数: object_type
- 返回: ObjectId
- 副作用: 新 Object 进入 BIRTH 状态，创建者获得 AdminCap
- 事务性: 是

### 3.2 OBJECT_DEATH
- 功能: 销毁一个 Object
- 参数: ObjectId
- 返回: 无
- 副作用: Object 进入 DEAD 状态，级联撤销所有 Capability，清理 Link
- 事务性: 是

### 3.3 OBJECT_LINK
- 功能: 在两个 Object 之间建立 Link
- 参数: from_ObjectId, to_ObjectId
- 返回: 无
- 副作用: 建立单向 Link
- 事务性: 是

### 3.4 CAPABILITY_GRANT
- 功能: 授予 Capability
- 参数: from_ObjectId, to_ObjectId, capability_type
- 返回: CapabilityId
- 副作用: 创建新的 Capability 边
- 事务性: 是

### 3.5 CAPABILITY_REVOKE
- 功能: 撤销 Capability
- 参数: CapabilityId
- 返回: 无
- 副作用: 级联撤销
- 事务性: 是

### 3.6 MEMORY_ALLOC
- 功能: 为 Object 的 Memory Space 分配新槽位
- 参数: ObjectId, size_hint
- 返回: StateId
- 事务性: 是

## 4. 内核与 Transaction 的关系

- 所有内核服务调用都在当前 Transaction 上下文中执行
- 内核服务的副作用在 Transaction 提交时生效
- Transaction 中止时，内核服务的副作用回滚

## 5. 内核 Object

内核本身是一个特殊的 Object:
- ObjectId = 0 (硬编码)
- ObjectType = KernelObject
- 拥有最高特权
- 不接受来自其他 Object 的 Capability 限制
- 用户代码通过 TRAP 指令调用内核，而非通过 Capability

## 6. Host Call 边界

某些能力 Veritas Machine 自身无法提供，需要外部环境支持。
这些通过 Host Call 接口暴露。

Host Call 与 Kernel Service 的区别:
- Kernel Service: Veritas Machine 内部实现
- Host Call: 由 Veritas 之外的环境提供

Host Call 列表:

| 调用 | 功能 | 当前实现 |
|---|---|---|
| host_time | 获取当前时间戳 | Rust std::time |
| host_random | 获取随机字节 | Rust rand |
| host_write | 向外部输出 | stdout/stderr |
| host_read | 从外部输入 | stdin |
| host_spawn | 创建外部进程 | 未实现 |

调用方式: TRAP <host_call_id>

## 7. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| TRAP 机制 | machine.rs trap_frame | 保持 |
| Kernel Service | engine.rs pub fn 方法 | 转为 TRAP 处理 |
| Host Call | 散落在各处 | 统一接口 |
| 内核 Object | 不存在 | 新增 ObjectId=0 |

## 8. 实现要求

1. 所有内核服务通过 TRAP 调用，不可通过函数调用
2. 内核服务必须事务性
3. Host Call 必须统一收口，不可散落
4. 内核 Object 硬编码为 ObjectId=0
'''

with open('docs/constitution/kernel.md', 'w') as f:
    f.write(content)
print('Done: docs/constitution/kernel.md')
