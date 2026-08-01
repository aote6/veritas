# Veritas Link Specification v0.1

## 1. Link 是什么

Link 是 Object 之间的命名关系。不是裸边，不是图论意义上的连接。

Link 定义了 Object A 知道 Object B 存在，并且这种"知道"带有
明确的语义和级联行为。

## 2. Link 的结构

Link:
- from: ObjectId (Link 的起点)
- to: ObjectId (Link 的终点)
- link_type: LinkType (语义类型)

Link 是单向的。双向关系需要两条 Link。

## 3. LinkType 定义

### 3.1 DEPENDS_ON

含义: from 依赖 to。from 的功能需要 to 的存在才能正常运行。

级联行为:
- to 死亡: from 收到 TRAP (通知信号)，from 可以选择处理或自行销毁
- from 死亡: Link 清理，to 不受影响
- to 冻结: from 不可修改 to，但可以读取
- to 进入 DEAD 后: Link 自动清理

使用场景:
- ModuleInstance 依赖 ModuleObject
- 一个 StateObject 存储了指向另一个 StateObject 的引用

### 3.2 OWNS

含义: from 拥有 to。to 的生命周期由 from 控制。

级联行为:
- to 死亡: from 收到 TRAP
- from 死亡: to 级联死亡 (OBJECT_DEATH)
- to 冻结: from 不可修改 to
- to 不可独立存活: 没有 from 存在时，to 没有存在的意义

使用场景:
- ModuleInstance 拥有其创建的临时 StateObject
- 容器 Object 拥有其内部的子 Object

### 3.3 REFERENCES

含义: from 引用 to。一种弱关系，主要用于查询和遍历。

级联行为:
- to 死亡: Link 断开，from 不受影响 (不收到 TRAP)
- from 死亡: Link 清理，to 不受影响
- to 冻结: 无影响
- 最弱的关系类型

使用场景:
- 缓存指向
- 索引 Object 指向被索引 Object
- 历史记录中的引用

## 4. Link 操作

### 4.1 OBJECT_LINK

指令: OBJECT_LINK from, to, link_type

前置条件:
- from 和 to 必须存在且存活 (ACTIVE 或 FROZEN)
- from 不等于 to (拒绝自环)
- 同方向同类型的 Link 不可重复 (幂等)

后置条件:
- 创建 from -> to 的 Link

### 4.2 OBJECT_UNLINK

指令: OBJECT_UNLINK from, to

前置条件:
- Link 必须存在

后置条件:
- 删除 from -> to 的 Link
- 不触发任何级联行为 (Link 的删除本身是安全的)

## 5. Link 与 Transaction

- OBJECT_LINK 在 Transaction 内执行
- Transaction 提交时 Link 生效
- Transaction 中止时 Link 回滚
- Link 的建立和删除都是事务性的

## 6. Link 与 Object 生命周期

Object 死亡时的 Link 清理顺序:

1. 遍历该 Object 的 incoming_links (别人指向它的 Link)
2. 对每条 incoming_link，根据 link_type 决定行为:
   - DEPENDS_ON: 通知 from，然后删除 Link
   - OWNS: 通知 from，from 已死则 to 级联死亡
   - REFERENCES: 直接删除 Link
3. 遍历该 Object 的 outgoing_links (它指向别人的 Link)
4. 对每条 outgoing_link，直接删除 Link
5. Object 从 ObjectRegistry 移除

## 7. Link 的查询

Machine 提供内核服务用于查询 Link:

- GET_INCOMING_LINKS ObjectId -> [Link]
- GET_OUTGOING_LINKS ObjectId -> [Link]
- HAS_LINK from, to -> bool

这些是只读查询，不需要 Transaction。

## 8. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| Link 结构 | engine.rs topology | 独立为 Machine 组件 |
| OBJECT_LINK | engine.rs object_link | 转为 ISA 指令 |
| OBJECT_UNLINK | 不存在 | 新增指令 |
| LinkType | 不存在 | 新增，三种类型 |
| 级联行为 | 部分在 object_death | 按 LinkType 分发 |
| 查询服务 | 不存在 | 新增 Kernel Service |

## 9. 实现要求

1. Link 必须携带 LinkType，不可作为裸边
2. 自环 Link 必须拒绝
3. Object 死亡时严格按 LinkType 执行级联行为
4. Link 操作必须事务性
5. 同方向同类型 Link 不可重复
