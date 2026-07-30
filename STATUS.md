=== Veritas Kernel v0.1.0 ===

标签: v0.1.0
日期: 2026-07-29
提交: 8590434

## 已实现

### 内核原语
- BEGIN / COMMIT / ABORT（OCC冲突检测+快照隔离+ReadFutureVersion）
- READ / WRITE（读写屏障+版本追踪）
- EFFECT（幂等副作用队列+EffectAck）
- SAVEPOINT / ROLLBACK_TO（含pending_objects和pending_links回滚）
- OBJECT_BIRTH（唯一性排他+零痕迹ABORT+WAL持久化+全局注册表）
- OBJECT_LINK（自环防御+拓扑固化+WAL持久化+ABORT零痕迹）

### 运行时服务
- WAL：序列化/反序列化/崩溃恢复/Effect重试
- ScopeRegistry：struct_version独立维护+幻读防御
- CapabilityGraph：主权已迁移至ObjectId
- StateStore：版本化状态存储

### 崩溃恢复
- WAL Replay重建state_map+scope_map+object_registry+topology
- tx_id续接
- Effect重试+去重

## 测试
- 56个测试全部通过
- 测试隔离：每个测试独立WAL路径

## 待实现
- 重复Link去重策略
- 并发Link OCC冲突检测
- 测试辅助函数抽取（new_test_engine）

## 规范
- docs/Veritas_Runtime_Object_规范.md（V1.0物理定版）

---

## P8 阶段：生命周期管理（2026-07-30 完成）

### P8.1 OBJECT_DEATH 原语
- object_death() 物理原语：Alive 检查 + pending_deaths 暂存
- ObjectState 枚举：Alive / Dead
- object_registry 从 HashSet 升级为 HashMap<ObjectId, ObjectState>
- WAL 支持 ObjectDeath 条目的序列化/反序列化/恢复
- Savepoint 回滚支持 pending_deaths 截断
- 5 个测试：正常死亡、拒绝未知对象、拒绝重复死亡、Savepoint 回滚、WAL 恢复

### P8.2 确定性拓扑清理
- commit 时清理所有涉及已死亡 Object 的拓扑边
- WAL 恢复时同步清理关联边
- 4 个测试：出边清理、入边清理、死亡后拒绝建连、恢复清理

### P8.3 CAPABILITY_GRANT 原语
- capability_grant() 原语：Alive 检查 + CapabilityGraph 集成
- CapabilityGraph 嵌入 VeritasEngine
- 3 个测试：正常授权、拒绝死对象、拒绝不存在对象

### P8.4 能力级联撤销
- ObjectDeath 触发能力级联作废
- deactivate_subtree 修复：先停用自身再递归子树
- grants 表同步清理
- 2 个测试：级联撤销、无关能力保留

### P8-final Ghost Data 斩草除根
- revoke_holder() 原子撤销：grants + holders + children + edges 四表一致性
- purge_subtree_strictly()：递归清除子树所有痕迹
- WAL 恢复时重放能力撤销
- 2 个测试：零幽灵数据、恢复一致性

### P8.5 ObjectGuard 防线收口
- 新建 src/guard.rs：统一生命周期校验入口
- 新建 src/view.rs：TransactionObjectView 融合 registry + pending
- ObjectGuard 方法：ensure_alive / ensure_dead / ensure_exists / ensure_not_exists / ensure_linkable / ensure_can_grant
- 四个原语的手写校验全部替换为 ObjectGuard 调用
- 72 个测试保持绿灯

## P9 阶段：Controller 架构（2026-07-30 完成）

### P9.2 LockManager
- 新建 src/lock.rs：ObjectId 粒度 Shared / Exclusive 锁
- 两阶段锁 (2PL)，commit/abort 自动释放
- 挂载到 object_birth / object_death / object_link 原语
- 5 个自有测试全部通过

### P9.3 TransactionManager (PCB)
- 新建 src/tx_manager.rs：独占 TxId 分配
- 维护 Active / Committed / Aborted 状态表
- WAL 恢复时接续 max_tx_id + 1
- 并发 1000 个 TxId 唯一性测试通过

### P9.4 Wound-Wait 死锁预防
- LockManager 持有 TransactionManager 引用
- 锁冲突时查询 PCB 比较新老：
  - 老事务抢新事务 -> Wound: 击毙新事务，夺锁 + release_all
  - 新事务抢老事务 -> Die: 当前事务避让
- Shared + Shared 兼容模式
- Engine::commit() 入口查询 PCB 状态，被击毙事务拒绝提交
- 7 个 LockManager 测试 + 状态传播验证

### P9.5 PCB 生命周期闭环
- commit/abort 后清理 PCB (remove)，防止长期运行泄漏
- is_aborted() / remove() 状态查询与清理接口
- 三层物理验证：
  1. Wound -> PCB 标记 Aborted -> commit 被拦截
  2. Wound 后锁释放 -> 后续事务可立即获取
  3. Aborted 事务未写 WAL -> 重启后零幽灵数据
- PCB 生命周期测试：commit 后移除、abort 后移除

### P9 阶段关键成果
- 90 个测试全部通过
- Engine 剥离 tx_id_counter，退化为纯原语执行器
- TxId 恢复续接验证通过
- WAL 垃圾文件已清理
- 控制权架构成型：TransactionManager -> LockManager -> Engine 完整闭环
