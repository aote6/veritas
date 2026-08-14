# Veritas 下一步推进计划

最后更新: 2026-08-14

## 当前状态
- cargo test: 331 passed
- Forge pytest: 210 passed（含 P0 CapabilityGrant 2 个 e2e）
- 多对象事务: birth A → write A → birth B → link A→B → commit → WAL recovery 全部通过
- capability_context bootstrap 修复完成
- P0 CapabilityGrant 链路闭合 ✅

## 已完成

### 1. veritasd 暴露 CapabilityGrant 命令（P0）✅ 2026-08-14 确认

**veritasd**: src/bin/veritasd.rs:292 已有 tx_capability_grant JSON 命令
**Forge adapter**: forge/world/adapter.py:249 已有 tx_capability_grant() 方法
**Forge session**: forge/world/session.py:70 已有 grant() 方法
**测试**: tests/test_p2_capability_grant.py 2 passed

验证（2026-08-14）:
python -m pytest tests/test_p2_capability_grant.py -v
2 passed in 0.25s

---

## 待完成

### 2. world_demo.vasm 重写（P1）

**原因**：上次 identity 模型收紧后 world_demo 失效了。现在 VASM 的 Operand 支持动态寄存器、CALL 路径的 capability 验证已修好、多对象事务已通。重写 demo 能一次性验证整条链路。

**解决**：写新的 world_demo.vasm，覆盖 birth A → CALL 进 A → WRITE → RETURN → birth B → CALL 进 B → WRITE → RETURN → LINK A→B → COMMIT → HALT

**步骤**：
1. 确认 CALL/RETURN 的寄存器传递语义
2. 写 world_demo.vasm
3. compile + run + inspect 全链路验证
4. 确认 WAL recovery 后状态一致

---

### 3. 跨对象事务的排列组合测试（P1）

**原因**：目前只测了一种操作顺序。需要覆盖更多变体。

**原则**：只增加测试，不修改内核。若测试失败，先分类：
- ① 真 bug → 修
- ② 测试假设错误 → 修测试
- ③ 架构边界导致的预期失败 → 确认预期后标记
- ④ Session/Machine 架构债 → 不趁机重构

**第一批测试清单**：

正常路径：
1. birth A → birth B → write A → write B → commit
2. birth A → birth B → link A→B → write A → commit
3. birth A → birth B → write A → link A→B → commit

Grant 路径：
4. birth A → birth B → grant A→B → write B → commit
5. birth A → birth B → grant A→B → link B→C → commit
6. birth A → birth B → grant A→B → write B → link B→A → commit

Abort 路径：
7. birth A → write A → birth B → write B → abort（确认回滚）
8. birth A → birth B → grant A→B → abort → capability 不应留下

Recovery 路径：
9. grant → write/link → commit → WAL recovery → capability + object/link 状态一致
10. grant → abort → WAL recovery → 不应出现残留 capability

反向测试：
11. A grant B on C → B 操作 C 成功 → 确认 A 不因此变成 holder
12. A grant B → B 再尝试 grant C → 验证授权链语义不变

---

### 4. Session 和 Machine 行为一致性测试（P2）

**原因**：两条入口路径各自维护身份上下文，防止再次分化

**步骤**：
1. 选一个简单序列：birth A → write A → commit
2. Session API 跑一次
3. Machine 指令跑一次
4. 断言两者产生的 receipt 一致

---

### 5. 架构统一：Session 底层走 Machine（P3）

**原因**：当前 Session 手抄了半份 CALL 逻辑，是身份 bug 反复出现的根源。

**解决**：抽取出 enter_identity/leave_identity，Session 的 tx_write 改成 enter→write→leave 对称模式

**时机**：不紧急。等 2-4 完成后再执行。

---

## 顺序
2 → 3 → 4 → 5

## Stage 2（World State 完整性，world.md §12 主线）

在 2-5 完成后启动：
1. WorldSnapshot 扩展为八组件
2. StateEntry 真实 version
3. Object Death 清理 StateStore
4. Checkpoint 保存/恢复完整 Machine State
5. Recovery 恢复计数器续接
6. Replay 统一走 TransactionDelta → apply()（P30.4/P30.5）
7. root_hash 升级 SHA-256
