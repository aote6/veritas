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
- to 死亡: 系统发出 DependencyInvalidated（dependent=from, dependency=to）
  - 载体可以是 Effect、Trap 或其它可恢复通道；宪法只规定事件语义，不规定载体
  - from 保持 Alive，不级联死亡
  - Link 删除
- from 死亡: Link 清理，to 不受影响
- to 冻结: from 不可修改 to，但可以读取

使用场景:
- ModuleInstance 依赖 ModuleObject
- 一个 StateObject 存储了指向另一个 StateObject 的引用

### 3.2 OWNS

含义: from 拥有 to。to 的生命周期由 from 控制。

级联行为:
- from 死亡: to 级联死亡（OBJECT_DEATH，传递闭包；见 expand_owns_death_closure）
- to 死亡: Link 删除；from 不因此死亡（所有权方向是 from→to，不反向级联）
- to 冻结: from 不可修改 to
- from 冻结: 不可再建立以该 from 为端点的新 OWNS（若实现层有额外约束，以代码为准）

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

Object 死亡在 **commit 边界**生效。设显式死亡请求经 OWNS 闭包展开后得到
完整集合 D（death set）。

对 D 的处理顺序（语义顺序，实现可合并步骤但不得改变结果）:

1. **OWNS 闭包（先于单点状态固化）**
   - 从待死亡集合出发，沿已提交拓扑与本事务 pending_links 中
     link_type = OWNS 且方向为 from → to 的边向外扩展；
   - pending_unlinks 中的边不参与传播；
   - 得到完整 death set D（传递闭包）。

2. **DEPENDS_ON（incoming：to ∈ D）**
   - 对每条 from --DEPENDS_ON--> to 且 to ∈ D：
     - 发出 DependencyInvalidated(dependent=from, dependency=to)
       （若 from ∉ D；from 已在 D 中则不必通知）
     - 删除该 Link
   - from 保持 Alive，除非 from 自身也在 D 中

3. **REFERENCES 与其余边**
   - 任一端点属于 D 的边：删除
   - 无通知、无级联死亡

4. **状态固化**
   - D 中每个 ObjectId：状态 → Dead（终态，不可逆）
   - ObjectId **不从 registry 回收、不可复用**（记录可保留为 Dead，
     而非"移除后 Id 消失"）

5. **Capability**
   - 不在此路径要求 eager 清扫 Capability 图
   - 以 D 中对象为 resource 的授权在**使用时**因 resource 非 Alive 而失败

说明:
- "通知"在 DEPENDS_ON 上的规范名是 DependencyInvalidated；载体
  （Effect / Trap / 其它）由实现选择，宪法不绑定单一载体。
- OWNS 的生命周期传播只沿 **owner → owned**，与 DEPENDS_ON 的
  "依赖方保持存活 + 收事件"严格区分。

## 7. Link 的查询

Machine 提供内核服务用于查询 Link:

- GET_INCOMING_LINKS ObjectId -> [Link]
- GET_OUTGOING_LINKS ObjectId -> [Link]
- HAS_LINK from, to -> bool

这些是只读查询，不需要 Transaction。

## 8. 当前实现映射

| 规范 | 当前代码 | 状态 |
|------|----------|------|
| OWNS cascade | expand_owns_death_closure | done P8.1 |
| DEPENDS_ON notification | DependencyInvalidated (commit boundary) | done P8.2 |
| REFERENCES | topology retain | done |
| Link structure | engine.rs topology | done |
| OBJECT_LINK | engine.rs object_link | done |
| OBJECT_UNLINK | pending_unlinks | done |
| Carrier (Effect/Trap) | observation port + Effect convention; carrier evolvable | partial |

## 9. 实现要求

1. Link 必须携带 LinkType，不可作为裸边
2. 自环 Link 必须拒绝
3. Object 死亡时严格按 LinkType 执行级联行为
4. Link 操作必须事务性
5. 同方向同类型 Link 不可重复
