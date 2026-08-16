# Veritas 下一步推进计划

最后更新: 2026-08-16

## 当前状态

- cargo test: 245 passed, 0 failed
- Verification Map: 245/245, Phase 1 + Phase 2 PASS
- Checkpoint Integrity / Commitment Closure: FROZEN
- Replay Continuity (P30.4 / P30.5): CLOSED
- Global Architecture Audit: CLOSED（无 BLOCKER / 无新 MAJOR）

## 已完成

### Checkpoint Integrity 主线 ✅
- Phase 0: 只读审计
- Phase 1: ADR + Q1-Q4 裁定
- Phase 2A: tx_id 移除 + commitment_hash → state_commitment
- Phase 2B: State Commitment FNV-1a → SHA-256
- Phase 2C: Checkpoint Commitment Verification
- Phase 2D: Delta Identity FNV-1a → SHA-256
- 长度前缀 + Commitment Domain 边界文档

### Stage 2（World State 完整性）✅ 全部完成
1. WorldSnapshot 八组件 ✅
2. StateEntry 真实 version ✅
3. Object Death 清理 StateStore ✅
4. Checkpoint 保存/恢复完整 Machine State ✅
5. Recovery 计数器续接 ✅
6. Replay 统一 TransactionDelta → apply() ✅
7. root_hash SHA-256 ✅

### 测试基础设施 ✅
- meta_verification_comments 强制所有测试有注释
- gen_verification_map_fixed.py 生成带元数据的验证地图
- check_verification_map.py Phase 1/2 全 PASS

## 已知未实现（设计如此或非阻塞）

- MemoryAlloc：KernelCall 到达即空操作
- Effect executor：recovery 保持 pending，无自动重放执行
- Machine 完整寄存器/栈状态不进 WorldSnapshot（checkpoint 目标是 World State）
- TRAP Savepoint/RollbackTo 名恒空（ABI 未完成）
- grant → WAL recover → 新 session 用 recovered grant（可选增强）

## 下一阶段：Forge ↔ Veritas Identity / Capability Boundary Closure

先审计，不直接写 adapter。

审计对象：
  Forge agent/task 身份
    ↓
  grantee / grantor / resource / capability_id
    ↓
  veritasd JSONL
    ↓
  WorldService
    ↓
  AccessIntent
    ↓
  Kernel authorization

核心问题：
  Forge 有没有可能把"谁在操作"弄丢、弄错、降级或绕过？
