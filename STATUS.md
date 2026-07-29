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
- OBJECT_DEATH（级联回收+拓扑断开+墓碑标记+ABORT复活）
- 重复Link去重策略
- 并发Link OCC冲突检测
- 测试辅助函数抽取（new_test_engine）

## 规范
- docs/Veritas_Runtime_Object_规范.md（V1.0物理定版）
