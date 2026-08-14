# Veritas Architecture Debt / Constitution Drift 审计报告

**审计日期**: 2026-08-14  
**审计范围**: 代码考古 + 架构审计 + 宪法一致性审计  
**约束**: 本轮**禁止**修改任何 `src/` 代码、测试逻辑、Constitution；唯一产物为本报告。  
**数据来源**: `docs/VERIFICATION_MAP.md`、`docs/IDENTITY_MODEL.md`、`docs/constitution/*`、`ROADMAP_NEXT.md`、`STATUS.md`、`src/**`、`tests/**`、`bin/**` 的阅读与调用关系检索。

---

## 0. 执行摘要

Veritas **已经是一个自洽、可运行的计算机内核原型**：Transaction、Identity（current_object / capability_context）、Capability（authorize_intent + graph）、WAL/Recovery/Replay、Machine→KernelCall→Engine、WorldService/veritasd JSONL、world_demo.vasm 垂直证明均有活跃路径与回归测试锁定。

本轮**不是**找 bug 清单，而是把：

- **真正活着的核心架构**
- **历史遗留 / 兼容层**
- **死代码**
- **与 Constitution / IDENTITY_MODEL 的 drift**
- **Constitution 要求但未实现（且非 Future Extension）的项**

分离清楚。

**不接受**“80–85% 完成度”这类主观百分比作为结论。

### 分类统计（重要发现项）

| 分类 | 数量（约） | 说明 |
|------|-----------|------|
| A. ACTIVE CORE | 12+ | 主执行/身份/能力/事务/WAL 主链 |
| B. ACTIVE BUT DRIFTED | 4 | 仍在用，与当前宪法/身份模型有偏差 |
| C. LEGACY | 6 | 旧路径/旧抽象，仍有调用或兼容用途 |
| D. DEAD CODE | 3 | 无外部活跃调用路径 |
| E. CONSTITUTION GAP | 3 | 宪法明确要求且非 Future、实现缺失或半成品 |
| F. FUTURE EXTENSION | 3 | 宪法已标未来扩展，不算当前缺陷 |
| G. TEST-ONLY | 2 | 仅测试入口 |
| H. UNCERTAIN | 2 | 证据不足 |

**清理优先级（仅建议，本轮不执行）**

| 优先级 | 数量 | 含义 |
|--------|------|------|
| P0 | 2 | 可能影响安全/语义一致性的 drift（需评估，非立即改 Kernel） |
| P1 | 5 | 双路径 / 明显 legacy，易导致未来误判 |
| P2 | 5 | 死代码、无用 wrapper、过时文档 |
| P3 | 3 | 纯整理 |

**关键结论预览**

- 发现 **Constitution Drift**：**是**（主要在 Identity 外部 API 入口、Module 模型、TRAP 完整性、root_hash 强度、commit_version 部分条款）。
- 发现 **明显旧架构残留**：**是**（enter_object 作为原语仍存在；Controller/TxManager 与 Kernel 事务路径并存；Savepoint 已实现但宪法标 Future；executor 已删除但文档仍提）。
- 发现 **死代码**：**是**（`Extension` trait 无实现者；部分小型模块调用面极窄）。
- 是否发现**真正需要立即修改 Kernel 的问题**：**否**（P0 为 drift 评估项；Forge 当前主路径已走 authorize_intent；冻结安全结论不重开）。

**P0 Identity Drift 收口** (2026-08-14): 经 `IDENTITY_DRIFT_AUDIT_20260814.md` 定界后，**不是 Kernel security bug**。已通过 `IDENTITY_MODEL.md` §7 + 源码定位注释 **RESOLVED BY ARCHITECTURAL DECISION / DOCUMENTATION**（未改 authorize_intent / CALL / BIRTH / capability_context 逻辑）。

---

## 1. 当前真正核心执行路径

```
.vasm 源码
  → assembler.rs (assemble / assemble_module)
  → instruction.rs + instruction_codec.rs (Instruction / 字节码)
  → machine.rs Machine::boot / step / run
       ├─ 本地指令: Add/Sub/Cmp/Jmp/LoadConst/…（不碰 Kernel）
       └─ 需特权的指令:
            构造 KernelCall → kernel.rs Kernel::handle()
              → engine.rs VeritasEngine（object_birth / link / grant / authorize_intent / commit…）
  → TransactionContext（types.rs）: current_object, capability_context, capabilities, pending_*
  → COMMIT: engine.commit → WAL（wal.rs）写入 TransactionDelta / WalEntry
  → Recovery: Kernel::with_wal_path / WAL replay → 重建 object_registry / topology / capability_graph / StateStore
```

**外部系统路径（Forge / veritasd）**

```
JSONL 请求
  → bin/veritasd.rs
  → world_api.rs WorldService (tx_begin / tx_* / tx_commit / tx_capability_grant …)
  → Kernel::handle / engine.authorize_intent / engine.commit
  → 同一套 WAL / Recovery
```

**模块级便捷路径**

```
Runtime::execute(kernel, ModuleImage)
  → Machine::new → boot → step 循环
```

**事实**: `executor.rs` 已不存在于树中；`ARCHITECTURE_INVENTORY.md`（2026-08-04）仍列出 executor，**文档滞后**。IDENTITY_MODEL 中“Executor vs Machine 重叠”描述过时。

---

## 2. 当前真正的身份模型

依据 `docs/IDENTITY_MODEL.md`（2026-08-10）与代码：

| 概念 | 归属 | 行为 |
|------|------|------|
| `current_object` | TransactionContext | 地址空间归属；Read/Write 默认作用于此 Object |
| `capability_context` | TransactionContext | Capability 检查所用身份；CALL 切换，RETURN 从 CallFrame 恢复 |
| `ctx.capabilities` | **transaction-scoped** | attach 的 capability_id 列表，非 identity-scoped |
| 身份切换合法入口 | **CALL** | 先 `authorize_intent(AccessIntent::Call)`，通过后改 current_object + capability_context |
| OBJECT_BIRTH | 不切换身份 | 创建后把 self-AdminCap **attach** 到本事务 ctx，使 CALL 可审计通过 |
| OBJECT_LINK | 不隐式 enter_object | 经 authorize_intent(Link) |
| RETURN | 恢复 CallFrame 中身份 | 无需再审计 |

**辅助原语仍存在**: `TransactionContext::enter_object`（types.rs:317）仅设置 `current_object`；`Machine::set_execution_object` 同时设 current_object 与 capability_context（测试/引导用）。

---

## 3. 当前真正的 capability authorization 路径

**唯一事实来源**: `engine.rs` `authorize_intent`（约 391 行起）。

规则要点（与 kernel.md Self-Access Exemption 一致）：

- `target == current_object || target == capability_context` → 结构性豁免，不查 graph
- 否则查 capability_graph.holds / pending_capabilities + ctx.capabilities 等

**活跃调用者**:

- `machine.rs` CALL 路径
- `world_api.rs` 跨对象 freeze / death / grant 前切换、tx_write 跨对象前
- `engine` 内部 verify 路径（commit 时）
- `test_api` 测试包装

**CAPABILITY_GRANT 四层**（与冻结事实一致）: Instruction / KernelCall / Engine.capability_grant / WorldService.tx_capability_grant + veritasd JSONL。

---

## 4. 当前真正的 transaction / session 路径

| 层 | 入口 | 说明 |
|----|------|------|
| Kernel/Engine | `begin` / `begin_in_object` / `commit` / `abort` | 核心事务生命周期 |
| Machine | 持有单个 `ctx`，CALL 不新建事务 | 与 transaction.md 一致 |
| WorldService | `tx_begin(actor)` → SessionState { ctx, actor }；`tx_commit` / `tx_abort` | Session = 外部对单事务的句柄；session 隔离已验证 |
| Controller + TxManager + Lock | engine 内部 commit 路径仍构造使用 | 偏早期事务框架残留，但 **仍被 engine 调用** → 非死代码 |

不可嵌套、读写集 Address=(ObjectId,StateId)、跨 Object 同一事务：与 constitution 一致。

---

## 5. 当前真正的 WAL / recovery 路径

- **写入**: commit 时 pending_* → WalEntry / TransactionDelta → `wal.rs`
- **恢复**: `Kernel::with_wal_path` 等 → replay → 重建 registry / topology / cap graph / state
- **Checkpoint**: WorldSnapshot 五（+）组件 roundtrip；测试 `checkpoint_*`、`wal_recovery_*`、`replay_*` 覆盖
- **Determinism**: 同 WAL → 同 Engine state（P30 系列）

**未发现**第二套独立的生产 Recovery 实现。旧 `replay.rs` / `checkpoint.rs` 等已在 ARCHITECTURE_INVENTORY 中标为待删除且当前树中不存在对应生产模块（inventory 部分过时但方向正确）。

---

## 6–10. 重要发现清单（带证据）

### 发现 #1 — `enter_object` 仍为公开切换原语

- **文件**: `src/types.rs` ~317  
- **函数**: `TransactionContext::enter_object`  
- **分类**: **C. LEGACY**（原语）+ 在 WorldService 中曾构成 **B. ACTIVE BUT DRIFTED**（见 #2）  
- **当前调用者**: `engine.begin_in_object`、`world_api` 多处、`machine.set_execution_object`、测试  
- **当前行为**: 仅 `self.current_object = object_id`（**不**改 capability_context）  
- **对应文档**: IDENTITY_MODEL §7 — 执行期唯一切换入口是 CALL；`enter_object` = internal primitive  
- **为何此分类**: 作为底层赋值原语仍需要；但作为“业务级身份切换 API”已被 CALL 取代  
- **影响 Forge**: WorldService 路径需单独看（#2）  
- **是否需改 Kernel**: 否  
- **决议** (2026-08-14): **KEEP INTERNAL** — 源码注释 + IDENTITY_MODEL §7.4 已定位；禁止新执行期无审计路径。

---

### 发现 #2 — WorldService 在 OBJECT_BIRTH（actor=0）后隐式 `enter_object`

- **文件**: `src/world_api.rs` ~330–338  
- **行为**: `tx_create_object` 在 `current_object == 0` 时对新生 id 执行 `enter_object` + `capability_context = id`  
- **分类**: **B. ACTIVE BUT DRIFTED** → **已收口为文档化 Bootstrap Exception**  
- **对照**: IDENTITY_MODEL / Machine 路径 — OBJECT_BIRTH **不**切换身份，只 attach self-AdminCap，由 CALL 进入  
- **缓解**: 其它跨对象路径（tx_write / freeze / death / grant）已在 enter 前 `authorize_intent`（与 HIDDEN_ISSUES 旧描述相比已部分修复）  
- **影响 Forge**: Forge 依赖 create 后 bootstrap 为 working object；与 Machine 不同构但作用域分离  
- **是否需改 Kernel**: **否**  
- **决议** (2026-08-14): **DOCUMENT AS BOOTSTRAP EXCEPTION** — 见 `IDENTITY_MODEL.md` §7.3；`IDENTITY_DRIFT_AUDIT_20260814.md` Decision 1。**P0 CLOSED**。

---

### 发现 #3 — `begin_in_object` 无审计切身份

- **文件**: `src/engine.rs` ~908–912；`kernel.rs` 转发  
- **分类**: **C. LEGACY** / 引导 API → **已正式定义为 Session Bootstrap**  
- **行为**: `begin()` 后直接 `enter_object(object_id)`，不走 CALL  
- **调用者**: WorldService `tx_begin(Some(actor))`、kernel 内测、部分集成测试  
- **语义**: 打开“已在该 object 身份下”的事务，而非执行中切换 — 与 CALL 不同构  
- **决议** (2026-08-14): **OFFICIAL SESSION BOOTSTRAP** — IDENTITY_MODEL §7.5 + 源码注释；非 CALL、非执行期 identity switch。

---

### 发现 #4 — Machine CALL / RETURN 与 authorize_intent

- **文件**: `src/machine.rs` ~480–520；ObjectBirth attach ~600–650  
- **分类**: **A. ACTIVE CORE**  
- **证据**: CALL 构造 `AccessIntent::Call` → `authorize_intent`；失败 → Trap AccessDenied；成功压 CallFrame 并切换 current_object + capability_context  
- **OBJECT_BIRTH**: 不 enter；attach pending AdminCap — 与 IDENTITY_MODEL 一致  
- **建议**: **DO NOT TOUCH**

---

### 发现 #5 — KernelCall / Kernel::handle 主入口

- **文件**: `src/kernel.rs` KernelCall 枚举 + `handle`；`decode` TRAP ABI  
- **分类**: **A. ACTIVE CORE**  
- **说明**: Phase 1 仍是 Engine 薄包装；Machine 对多数特权指令已走 handle，而非任意直接 Engine 业务 API  
- **Drift**: constitution/kernel.md 要求“所有内核服务通过 TRAP”；Machine 仍有直接 `kernel.read/write/savepoint` 等路径 → **B** 见 #6

---

### 发现 #6 — Kernel TRAP 化不完整

- **事实**: TRAP 指令分支存在；`KernelCall::decode(service_id, r0,r1,r2)` 存在；但大量指令在 `machine.step` 中**直接**构造 `KernelCall` 或调用 `kernel.read/write`，并非一律经用户可见 TRAP 指令  
- **分类**: **B. ACTIVE BUT DRIFTED** 相对 kernel.md 理想模型；相对**当前工程阶段**可视为 **F/H**（“Phase 1 thin wrapper” 注释已承认）  
- **是否 Constitution 当前必需**: kernel.md §8 映射表写明 “Kernel Mode 不存在 / Kernel Service = engine 方法 → 转为 TRAP” — **方向性要求，非已冻结完成项**  
- **影响 Forge**: 否（Forge 走 WorldService）  
- **是否需改 Kernel**: **否（现在）**  
- **建议**: P1 文档澄清“指令级直接 KernelCall 为实现策略，TRAP ABI 为对外/跨模块目标”

---

### 发现 #7 — Host Call

- **文件**: `src/machine.rs` HostCall 分支 ~577–589  
- **行为**: call_id 0..=3 接受后 **空操作**（注释 P27 统一收口）；非法 id → Trap  
- **分类**: **E. CONSTITUTION GAP**（若要求真实 host_time 等）或 **F**（kernel.md 表：host_spawn 未实现；其余“当前实现”为 Rust 侧设想）  
- **判断**: 指令与编解码存在，**运行时能力未收敛到真实宿主** → 对内核原型 **非阻塞**；对“完整机器 + 非确定性宿主”为缺口  
- **影响 Forge**: 低  
- **建议**: P2 — 不阻塞 Forge；勿与 Kernel 安全主链混谈

---

### 发现 #8 — Savepoint / RollbackTo

- **文件**: instruction、codec、assembler、machine、engine.savepoint/rollback、KernelCall  
- **测试**: `capability_delegate_p4_recovery.rs` 使用 RollbackTo  
- **Constitution**: transaction.md §11 **明确** “Savepoint 是未来扩展，当前版本未实现” — **与代码冲突**  
- **分类**: **D 文档 drift** + 实现为 **A/C 混合**（已实现且被测，但宪法标 Future）  
- **正确归类**: **C/B** — 实现超前于已冻结的 “Future Extension” 表述  
- **建议**: P1 — **改 Constitution 状态**或标实现为 experimental；**不要删实现**（有测试依赖）

---

### 发现 #9 — root_hash = FNV-1a（u64）

- **文件**: `src/engine.rs` ~516–627 `root_hash` / `hash_each`  
- **Constitution**: world.md §9 — 当前 FNV-1a；**要求**密码学安全（SHA-256/BLAKE3）；算法可替换不进宪法正文细节  
- **ROADMAP_NEXT**: Stage 2 项 “root_hash 升级 SHA-256”  
- **分类**: **E** 相对第三方验证目标；**对内核可运行性与确定性回放**: **A**（确定性已满足）  
- **是否必须现在实现**: **否**（非 Forge 阻塞；属 Stage 2）  
- **建议**: P2/P3 路线图项；**不要**本轮改 Kernel

---

### 发现 #10 — ModuleObject / ModuleInstance

- **Constitution module.md**: ModuleObject 只读模板；执行需 ModuleInstance（StateObject）  
- **代码**: `module.rs` = `ModuleImage` + `ModuleLoader`（.vmod 文件格式）；`ObjectBody::Module { code_section, … }` 序列化存在；**无** ModuleInstance 运行时实体；执行 = Machine 直接 boot ProgramImage  
- **分类**: **E. CONSTITUTION GAP**（完整 Module 模型）+ 当前 **A** 为 “镜像加载 + 单机执行” 工程简化  
- **影响 Forge**: 低（当前不依赖 Instance 模型）  
- **建议**: P1 文档标注 “实现映射表已写明 Instance 不存在”；勿当生产 bug

---

### 发现 #11 — Extension trait

- **文件**: `src/extension.rs`  
- **调用者**: **无**（rg 无外部 use/impl）  
- **分类**: **D. DEAD CODE**  
- **建议**: P2 标记或未来删除；本轮不动

---

### 发现 #12 — Controller / TxManager / Lock

- **文件**: `controller.rs`, `tx_manager.rs`, `lock.rs`  
- **调用者**: `engine` commit 路径仍 `TransactionController::new`  
- **分类**: **A**（仍接入）但架构上偏 **C**（早期 OCC/锁框架与 WAL 主叙事重叠）  
- **建议**: P1 理清职责文档；禁止并行再写第二套事务状态机

---

### 发现 #13 — Verifier / Runtime / receipt-trace-event

- **Runtime**: `bin/veritas.rs`、`tests/kernel_world_runtime.rs` — **A** 薄封装  
- **Verifier**: `machine` boot 前 — **A** 轻量  
- **execution/trace/event/receipt**: Machine 记录路径使用 — **A**；与 World Receipt（commit Receipt）部分概念重叠 — **C** 文档层  
- **建议**: P3 命名区分 ExecutionReceipt vs Commit Receipt

---

### 发现 #14 — commit_version / Delta Identity 宪法

- **文件**: `docs/constitution/commit_version.md` 状态 “已冻结，待实现”  
- **代码**: WAL/commit 有 version 字段；WorldSnapshot 含 global_version、object_id_counter、grant_sequence（types.rs ~1010+）— 较 world.md §3 旧“缺失”表**已前进**  
- **分类**: **E** 完整 Delta Identity / apply 准入状态机未必闭环；**部分 A**  
- **建议**: P1 对照实现与 commit_version.md 做专项（非本轮）

---

### 发现 #15 — 过时文档与库存

- **ARCHITECTURE_INVENTORY.md**: 仍列 executor.rs、待删 state_memory 等 — 树中已无  
- **IDENTITY_MODEL** 末节: world_demo.vasm “失效”、tests/machine.rs 空 — **ROADMAP_NEXT / VERIFICATION_MAP 显示已推进**  
- **HIDDEN_ISSUES.md**: 部分 P0 描述未反映 world_api 已加 authorize_intent  
- **分类**: 文档 **C/D**  
- **建议**: P2 文档同步（非 Kernel）

---

### 发现 #16 — set_execution_object（测试入口）

- **文件**: `machine.rs` ~166–171  
- **调用者**: 主要为 `tests/call_access_intent.rs` 等  
- **分类**: **G. TEST-ONLY**  
- **建议**: 保持；勿用于生产 WorldService

---

## 6. 对“昨天 80–85% 报告”五项的逐项判断

| 说法 | 事实是否正确 | 是否当前 Constitution 必需 | 性质 | 现在是否应处理 | 影响 Forge | 需改 Kernel |
|------|-------------|---------------------------|------|----------------|------------|-------------|
| 1. root_hash = FNV-1a | **是** | 密码学强度：world 要求升级；确定性：已满足 | 技术债 / Stage 2 | 否（非阻塞） | 否 | 否（现） |
| 2. Kernel TRAP 化不完整 | **是**（理想模型 vs Phase 1） | 方向性；非已完成冻结项 | 架构演进 | 否 | 否 | 否（现） |
| 3. ModuleObject / Instance 半成品 | **是** | module.md 完整模型未落地；映射表已承认 | 功能缺口 / 简化实现 | 否 | 低 | 否（现） |
| 4. Host Call 未收敛 | **是**（空实现） | 非确定性宿主；非 World State | 缺口 / 非内核主链 | 否 | 低 | 否 |
| 5. Savepoint 未实现 | **错误** | 宪法写 Future 未实现，**代码已实现且有测试** | **文档 drift** | 应改文档状态 | 否 | **不要删代码** |

**“80–85%”**: 拒绝作为度量。应以 **主链是否自洽可运行 + 安全不变量是否测试锁定** 评价。

---

## 7. 必须回答的 10 个问题

### 1. 当前真正核心执行路径？

见 §1：VASM → assembler → Machine → KernelCall/handle → Engine → Transaction/pending → commit → WAL → recovery/replay。外部：veritasd → WorldService → 同一 Kernel。

### 2. 当前真正身份模型？

current_object + capability_context；capabilities transaction-scoped；切换唯一执行期入口 CALL+authorize_intent；BIRTH attach self-AdminCap 不切换；LINK 不隐式 enter。

### 3. 当前真正 capability authorization 路径？

`VeritasEngine::authorize_intent`；Machine CALL 与 WorldService 跨对象操作（已授权后）enter；commit 侧 verify。

### 4. 当前真正 transaction/session 路径？

Engine begin/commit/abort；WorldService session 包装单事务；CALL 同事务切换 object。

### 5. 当前真正 WAL/recovery 路径？

单一 wal.rs + commit 写入 + with_wal_path/replay；Checkpoint WorldSnapshot；无第二生产 Recovery 栈。

### 6. 明显 legacy 架构？

enter_object 原语；begin_in_object 无审计 bootstrap；Controller/TxManager 早期框架；文档中的 Executor；Savepoint 与宪法 Future 表述冲突；过时 ARCHITECTURE_INVENTORY / 部分 HIDDEN_ISSUES。

### 7. Dead code？

`extension.rs` Extension trait（无实现者）；可能的极窄 API 需再静态分析确认，但 **不以 warning/命名判死**。

### 8. 与 Constitution 的 drift？

- Identity：WorldService birth 隐式 enter vs IDENTITY_MODEL  
- kernel.md TRAP/Kernel Mode 理想 vs Phase 1  
- module.md Instance vs ModuleImage boot  
- transaction.md Savepoint Future vs 已实现  
- world.md root_hash 强度  
- commit_version.md 待完整落地  

### 9. 真正未实现但必须实现的？

**对“内核原型成立”**: 无阻塞项。  
**对宪法完整机器愿景**: ModuleInstance、密码学 root_hash、TRAP/Kernel Mode 纯化、Host 真实实现、commit_version 准入闭环 — 按 Stage 推进，**非本轮 P0 改 Kernel**。

### 10. 现在绝对不要动？

见文末 **DO NOT TOUCH**。

---

## 8. 清理优先级（仅建议，本轮不执行）

### P0（语义一致性 drift）— RESOLVED BY ARCHITECTURAL DECISION / DOCUMENTATION

**原问题**：WorldService `tx_create_object` 在 `current_object==0` 时隐式 enter，以及 `enter_object` / `begin_in_object` 与 IDENTITY_MODEL「唯一切换入口 = CALL」表述的边界不清。

**审计结论**（`docs/IDENTITY_DRIFT_AUDIT_20260814.md`）：**不是 Kernel security bug**。未发现未经 `authorize_intent` 的生产执行期身份绕过。Host bootstrap 与 Machine 执行期作用域不同；原表述未覆盖 Session 层。

**最终决策**（已写入 `docs/IDENTITY_MODEL.md` §7 与相关源码注释）：

1. `tx_create_object(current_object == 0)` → **DOCUMENT AS BOOTSTRAP EXCEPTION**
2. `enter_object` → **KEEP INTERNAL**（仅设 current_object；生产跨对象须先 authorize）
3. `begin_in_object` → **OFFICIAL SESSION BOOTSTRAP**（非 CALL）
4. `Machine::set_execution_object` → **KEEP TEST/BOOTSTRAP ONLY**
5. `CALL` → **ONLY EXECUTION-TIME IDENTITY SWITCH**

**状态**: **CLOSED**（文档 + 定位注释收口；未改 Kernel 安全逻辑）。  

### P1（双路径 / legacy 误判风险）

1. 文档化 enter_object / begin_in_object 为 bootstrap，禁止新的无审计业务路径  
2. Savepoint：更新 transaction.md 或标 experimental  
3. TRAP Phase 1 vs 宪法表述对齐（文档）  
4. Module 实现映射与 module.md 读者预期  
5. Controller/TxManager 与 WAL 叙事职责说明  

### P2（死代码 / 过时文档）

1. Extension trait  
2. ARCHITECTURE_INVENTORY / HIDDEN_ISSUES / IDENTITY_MODEL 末节过时段落  
3. HostCall 空实现注释澄清  
4. root_hash SHA-256 路线图（不实施）  
5. 测试-only API 清单  

### P3（整理）

1. Receipt 命名（execution vs commit）  
2. 注释与非 exhaustive match 卫生  
3. 纯格式与模块边界美化  

---

## 9. Veritas Architecture Status

| 子系统 | 状态 | 说明 |
|--------|------|------|
| Core execution | **ACTIVE** | Machine→KernelCall→Engine 主链完整 |
| Identity | **ACTIVE**（Machine + 已文档化 Host bootstrap） | 执行期 CALL 锁定；Host bootstrap 已写入 IDENTITY_MODEL §7；P0 CLOSED |
| Capability | **ACTIVE** | authorize_intent + grant/delegate/revoke + WAL |
| Transaction | **ACTIVE** | begin/commit/abort；session 包装清晰 |
| WAL | **ACTIVE** | 提交路径写入完整 |
| Recovery | **ACTIVE** | 测试金字塔覆盖 |
| Determinism | **ACTIVE** | 同 WAL 同状态；hash 为 FNV 确定性 |
| Module | **INCOMPLETE** | ModuleImage 可跑；Instance 模型未落地 |
| TRAP | **DRIFT / INCOMPLETE** | ABI 与分支存在；非全量用户态 TRAP 门面 |
| Host Call | **INCOMPLETE** | 指令在，宿主能力空 |
| Version / Delta | **ACTIVE**（基础）；**INCOMPLETE**（commit_version 全文） | Snapshot 已含 counters；完整 Delta Identity 仍演进 |

### Veritas 当前是否已经形成一个自洽的、可运行的计算机内核原型？

**是。**

依据：垂直路径（VASM/Machine 与 veritasd/WorldService）可运行；身份与能力不变量有回归锁；事务原子性与 WAL 恢复/重放可验证；冻结的安全结论（见任务说明与 VERIFICATION_MAP）本轮不重开。未完成项主要是宪法愿景中的 ModuleInstance、密码学 commitment、TRAP 纯化与 Host 收敛 — **不否定原型已成立**。

---

## 10. DO NOT TOUCH

在无新的、更强证据与测试失败之前，**不要修改**：

1. `authorize_intent` 与 Self-Access 规则  
2. OBJECT_BIRTH self-AdminCap **attach** 语义（machine ObjectBirth 分支）  
3. OBJECT_LINK 去隐式 enter 的安全修复与 `machine_object_link_security`  
4. CALL/RETURN + CallFrame（current_object / capability_context）  
5. Capability grant/delegate/revoke 与 WAL 中 cap 条目  
6. `ctx.capabilities` transaction-scoped 语义  
7. Engine commit → WAL → recovery 主链  
8. WorldService 上已加的 “authorize 再 enter” 跨对象检查  
9. 已冻结的 VERIFICATION_MAP 安全相关测试意图  
10. 为通过测试而存在的 Savepoint/Rollback 实现（先改文档再谈删）

---

## 11. 审计过程备注

- 使用了目录阅读、`rg` 调用关系、关键文件精读；**未**修改任何 `src/` 或测试。  
- `cargo check` 因环境 `Cargo.lock` version 4 / toolchain 标志失败，**与本审计代码结论无关**；未强行改 lockfile。  
- 未把 “看起来旧 / pub(crate) / TODO 字样” 单独判死；dead 仅在确认无调用者时标注（Extension）。  
- 未把 Future Extension（如宪法原 Savepoint 表述）自动算作缺陷；反而标出 **实现与宪法 Future 标签反向 drift**。

---

## 12. 本轮产出

- **唯一变更**: `docs/ARCHITECTURE_DEBT.md`（本文件）  
- **源码变更**: 无  

*报告结束。*
