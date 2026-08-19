# Veritas Test Architecture

## 冻结版本：Baseline v1

---

## 测试分层

| 层 | 位置 | 数量 | 性质 | 进入 Verification Map |
|----|------|------|------|----------------------|
| L0 | `src/**` `#[test]` | 129 | Unit / Implementation / Regression | ❌ 否 |
| L1 | `tests/**` `#[test]` | 236 | System Verification / Frozen Guarantees | ✅ 是 |
| L2 | 未来独立 | 0 | Product / External E2E | 未来决定 |

---

## L0 定义

- 验证实现细节、内部状态机、格式边界
- 保护历史 bug 回归
- 可以依赖私有 API
- 不表达独立的系统 Guarantee
- **不进入 Verification Map**

## L1 定义

- 验证公开稳定路径的系统行为
- 每个测试必须有 Category / Layer / TestWorld / Requirement
- 对应已冻结的 Constitution Guarantee
- **进入 Verification Map，CI 强制完整**

## L2 定义（未来）

- 产品级端到端
- 需要独立环境/进程/网络
- 当前不存在，不单独拆目录

---

## Verification Map 纪律

**Map 不是测试数据库，是"已冻结、可对外主张的系统保证目录"。**

### 一个 src 单测升级到 L1 必须同时满足：

1. 独立系统不变量
2. 稳定公开路径
3. 当前 L1 无等价或更强覆盖
4. 有明确 Requirement 归属

### 禁止事项：

- CRITICAL 标签 ≠ 必须进 Map
- 测试重要性 ≠ Verification 层级
- 禁止为了覆盖率好看制造 Requirement
- 禁止因为"测试很重要"就塞进 Map
- 禁止为了补 Map 而新增伪缺口

---

## Requirement 纪律

- Requirement 必须对应 Constitution 冻结的 Guarantee
- 新增 Requirement 前必须证明现有 Map 存在真正缺口
- 同一 Requirement 可被多个 L1 测试共同证明

---

## 当前冻结状态

| 项目 | 状态 |
|------|------|
| L0 单元测试 | 129 个，冻结 |
| L1 系统验证 | 236 个，冻结 |
| Verification Map | 245/245，CI 强制 |
| L0→L1 升级 | 0 个 |
| P0 L1 保证缺口 | 无 |

---

## 待 Constitution 对齐项

- Kernel 结构体 = world authority 的表述尚未冻结
- Effect ACK/retry 承诺域待定
- 这些是 Constitution 决策，不是测试遗漏

