# Veritas 下一步推进计划

## 当前状态
- cargo test: 全部通过
- Forge pytest: 208 passed
- 多对象事务: birth A → write A → birth B → link A→B → commit → WAL recovery 全部通过
- capability_context bootstrap 修复完成

## 待完成

### 1. veritasd 暴露 CapabilityGrant 命令（P0）

**原因**：当前跨对象授权只能通过 ObjectBirth 自动授予创建者。如果两个独立创建的对象需要互相操作（比如 A 要写 B，但 B 不是 A 创建的），没有显式 grant 路径。

**解决**：veritasd 加 `tx_capability_grant` JSON 命令，Forge adapter/session 加对应封装。内核已有 `KernelCall::CapabilityGrant` 和 `Engine::capability_grant`，只需暴露。

**步骤**：
1. veritasd.rs 加 "tx_capability_grant" 命令处理
2. forge/world/adapter.py 加 tx_capability_grant 方法
3. forge/world/session.py 加 grant 方法
4. 写测试：独立创建 A 和 B，通过 grant 让 A 获得 B 的 capability，A 写 B 成功

---

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

**原因**：目前只测了一种操作顺序（birth A → write A → birth B → link A→B → commit）。需要覆盖更多变体，防止以后改代码时把身份生命周期搞坏。

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

**反向测试（重要）**：
11. A grant B on C → B 操作 C 成功 → 确认 A 不因此变成 holder
12. A grant B → B 再尝试 grant C → 验证授权链语义不变

---

### 4. Session 和 Machine 行为一致性测试（P2）

**原因**：两条入口路径各自维护身份上下文，防止再次分化

**解决**：同一个操作序列分别用 Session API 和 VASM 指令各跑一次，断言结果相同

**步骤**：
1. 选一个简单序列：birth A → write A → commit
2. Session API 跑一次
3. Machine 指令跑一次
4. 断言两者产生的 receipt 一致

---

### 5. 架构统一：Session 底层走 Machine（P3）

**原因**：详见 STATUS.md 中"架构债"记录。当前 Session 手抄了半份 CALL 逻辑，是身份 bug 反复出现的根源。

**解决**：抽取出 enter_identity/leave_identity，Session 的 tx_write 改成 enter→write→leave 对称模式

**步骤**：详见 STATUS.md

**时机**：不紧急。等 1-4 完成后再执行。

---

## 顺序
1 → 2 → 3 → 4 → 5

1 最堵路（没有 CapabilityGrant 多对象协作走不通），2 是验证全链路的标志性里程碑，3 和 4 是固化测试，5 是架构优化。
