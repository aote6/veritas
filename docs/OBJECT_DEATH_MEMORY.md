# Object Death Memory Reclamation

版本：v1.0
日期：2026-08-04
状态：已冻结

## 1. 宪法依据

**object.md §4**：Object 死亡后，ObjectId 永久保留，不可重用。

**memory.md §8**：Memory Space 的生命周期与 Object 绑定。Object 死亡时，其 Memory Space 被回收。

## 2. 硬件对应

进程死亡后：页表释放，内存回收，地址空间销毁。

不会留下"死亡进程的 RAM"。

Object Death → MemorySpace 回收是机器原语，不是 GC，不是优化。

## 3. 模型：死亡即物理删除

死亡 Object 的 StateStore 条目全部移除。

alive Object:
    ObjectRegistry ✓
    MemorySpace ✓
    Capability ✓

dead Object:
    ObjectRegistry: Dead（tombstone）
    MemorySpace: 空（所有 Address(object_id, *) 已删除）
    Capability: 已 revoke
    Topology: 相关边已删除

ObjectId 不回收，但 Memory 回收。

## 4. 实现规则

### 4.1 StateStore 新增方法

remove_object(object_id: ObjectId)：删除所有 Address(object_id, *) 的条目。

### 4.2 apply 顺序

在现有步骤 10 之后，新增步骤 11：

for each dead_obj in full_death_set:
    state_store.remove_object(dead_obj)

必须在 OWNS 闭包展开、Topology 清理、Capability revoke 之后执行。

### 4.3 对 root_hash 的影响

死亡 Object 的 StateStore 条目不再参与 root_hash。

同一个世界，同一组死亡 Object，得到同一个 root_hash。

不依赖"死亡前是否曾写入过某个 slot"。

### 4.4 对 Checkpoint 的影响

WorldSnapshot.state_entries 不含死亡 Object 的条目。

### 4.5 对 WAL 的影响

TransactionDelta 本身不变。WAL 记录的 writes 中可能包含死亡 Object 的写入，但 apply 时会先写后删——这不影响最终 World State。

## 5. 实现步骤

1. StateStore::remove_object(object_id)
2. apply() 增加步骤 11
3. 编写测试：Birth → Write → Death → 断言 StateStore 中不存在该 Object 的条目
4. 编写测试：OWNS 级联死亡 → 所有被级联 Object 的 StateStore 条目被清理
