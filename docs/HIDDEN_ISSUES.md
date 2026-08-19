# Veritas 潜藏问题分析报告

最后更新: 2026-08-19（状态标记更新）

本文档基于 2026-08-10 的代码审查，识别 Veritas 中尚未修复的潜藏问题。

分析方法论：从今天修复的 root / OBJECT_LINK 问题中提炼出的模式 ——
"修复漏洞时只删除了旧的隐式行为，却没有为新规则建立显式约束"。
按此模式系统性地搜索代码库，找到以下潜藏问题。

---

## 1. 问题总览

| 优先级 | 问题 | 位置 | 风险 |
|:---:|------|------|------|
| P0 | world_api.rs 4 处 enter_object 绕过 | world_api.rs:333,353,373,437 | **CLOSED by IDENTITY_DRIFT_AUDIT_20260814** |
| P0 | world_api.rs 注释承认的历史遗留 | world_api.rs:729-732 | **CLOSED by IDENTITY_DRIFT_AUDIT_20260814** |
| P1 | 314 个 unwrap() | 全项目 | **DEFERRED - 非阻塞** |
| P1 | capability_context 直接赋值 | machine.rs:146,429,438 | 权限混乱 |
| P2 | machine.rs 初始化 enter_object | machine.rs:145 | 隐式切换 |
| P2 | bypass 注释 | world_api.rs:432,677 | 设计不一致 |

---

## 2. P0：world_api.rs 的 enter_object 绕过

### 2.1 问题描述

world_api.rs 中有 4 处直接调用 enter_object() 切换身份，绕过了 CALL 的统一审计入口。

| 行号 | 代码 |
|------|------|
| 333 | state.ctx.enter_object(id); |
| 353 | state.ctx.enter_object(object_id); |
| 373 | state.ctx.enter_object(object_id); |
| 437 | state.ctx.enter_object(oid); |

### 2.2 和今天修复问题的关系

今天修复的 OBJECT_LINK 和 OBJECT_BIRTH 也是同样的问题 ——
隐式切换身份，绕过 CALL 审计。world_api.rs 的这 4 处调用是同一类问题。

### 2.3 风险

- 外部 API 可以任意切换身份
- 等价于系统后门
- 攻击者可以利用这个入口绕过所有 capability 检查

### 2.4 建议修复方案

将所有 enter_object 调用改为走 CALL 指令，经过 authorize_intent 审计。

---

## 3. P0：world_api.rs 注释承认的历史遗留

### 3.1 问题描述

第 729-732 行注释明确承认了 enter_object 的历史问题：

```rust
/// enter_object() before authorization. Object A without any capability
/// pin the specific historical bug: prior to the fix, enter_object was
```

### 3.2 风险

- 注释承认了问题，但代码没有修复
- 说明这个位置长期存在绕过行为

---

## 4. P1：314 个 unwrap()

### 4.1 问题描述

```
grep -rn "unwrap()" src/ --include="*.rs" | grep -v "test" | wc -l
314
```

### 4.2 风险

如果某个 unwrap() 在运行时遇到 None 或 Err，整个程序会 panic 崩溃。

### 4.3 需优先处理的位置

- machine.rs 里的 unwrap()（执行路径）
- engine.rs 里的 unwrap()（核心逻辑）
- kernel.rs 里的 unwrap()（内核入口）

### 4.4 建议修复方案

用 ? 操作符或 match 替换 unwrap()，让错误向上传播而不是崩溃。

---

## 5. P1：capability_context 直接赋值

### 5.1 问题描述

capability_context 在 4 处被直接赋值，与 current_object 同步切换：

| 行号 | 代码 |
|------|------|
| 146 | self.ctx.capability_context = object_id; |
| 429 | self.ctx.capability_context = object_id; |
| 438 | self.ctx.capability_context = frame.caller_capability_context; |

### 5.2 风险

capability_context 和 current_object 都是 authorize_intent 的豁免条件。
两者一起被切换，意味着 CALL 时两个身份一起变了。

潜在问题：
- 有没有地方只改了 current_object 没改 capability_context？
- 或者反过来？

### 5.3 建议修复方案

统一通过 enter_object 方法修改两者，确保原子性。

---

## 6. P2：machine.rs 初始化时的 enter_object

### 6.1 问题描述

machine.rs 第 145 行：

```rust
self.ctx.enter_object(object_id);
```

这个在 Machine::new() 里，应该是初始化时设置 root 身份。

### 6.2 风险

虽然目前只在初始化时使用，但不在 CALL 分支里，是隐式切换。
如果将来有人用这个初始化路径做别的事，可能绕过 CALL。

---

## 7. P2：bypass 注释

### 7.1 问题描述

代码中存在明确写着 bypass 的注释：

| 行号 | 内容 |
|------|------|
| 432 | // graph is bypassed entirely. |
| 677 | /// target == capability_context => skip capability graph entirely). |

### 7.2 风险

注释明确写着绕过 capability graph——说明设计上允许某些路径绕过权限检查。
这和今天修的问题同构——反正安全的隐式豁免。

---

## 8. 修复优先级

| 优先级 | 问题 | 预计工作量 |
|:---:|------|:---:|
| P0 | world_api.rs 的 4 处 enter_object | 1-2 小时 |
| P0 | world_api.rs 历史遗留注释 | 0.5 小时 |
| P1 | 314 个 unwrap() 替换 | 1-2 天 |
| P1 | capability_context 赋值审计 | 1-2 小时 |
| P2 | machine.rs 初始化 enter_object | 0.5 小时 |
| P2 | bypass 注释清理 | 0.5 小时 |

---

## 9. 下一步建议

1. **优先修复 P0**：world_api.rs 的 4 处 enter_object 绕过
   - 这是和今天修复问题同构的后门
   - 修复方式：删除 enter_object 调用，改为走 CALL 指令

2. **然后处理 P1**：314 个 unwrap()
   - 优先处理 machine.rs、engine.rs、kernel.rs
   - 用 ? 操作符替换，让错误向上传播

3. **最后清理 P2**：设计不一致问题
   - 统一 current_object 和 capability_context 的修改入口
   - 清理 bypass 注释

---

## 10. 相关文档

- docs/IDENTITY_MODEL.md — 执行身份与授权模型说明
- docs/BOOTSTRAP.md — 自举能力分析报告
- docs/constitution/world.md — World State Constitution v1.0
