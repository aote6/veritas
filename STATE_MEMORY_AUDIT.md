# State Memory Audit（Stage 0 地基核查）

日期：2026-08-04
状态：完成

## 结论

state_memory 已物理删除。不存在第二套状态源。Stage 0 通过。

## 证据

### 1. 源码搜索结果
- grep -rn "state_memory" src/ → 0 结果
- grep -rn "StateMemory" src/ → 0 结果
- 仅 1 处引用：tests/checkpoint_roundtrip.rs:95 过时注释（已删除）

### 2. 唯一状态源确认
- 唯一状态存储：src/store.rs StateStore
- 引擎唯一持有：engine.state_store: StateStore
- root_hash() 从 state_store.all_entries() 读取
- WAL 恢复通过 apply() 写入 state_store
- 读写路径全部指向同一个 StateStore

### 3. 判定
- [x] 0.1 数据一致性：state_store 是唯一状态源
- [x] 0.2 引用关系：全部引用点已分类，无代码依赖
- [x] 0.3 去留结论：state_memory 已废弃并物理删除

Stage 0 完成。可以进入 Stage 1。
