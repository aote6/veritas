# Veritas 下一步推进计划

最后更新: 2026-08-14

## 当前状态
- cargo test: 332 passed（331 + meta_verification_comments）
- Forge pytest: 210 passed（含 P0 CapabilityGrant 2 个 e2e）
- P0 CapabilityGrant 链路闭合 ✅
- P1-A world_demo.vasm Vertical Execution Proof ✅
- P1-B 跨对象事务矩阵 + Grant 闭环审计 ✅（VASM-LIMITED，跨 session 由 WorldService/veritasd 承载）
- 85 个缺失测试注释已补齐，meta 测试强制约束 ✅
- docs/VERIFICATION_MAP.md 自动生成 ✅

## 已完成

### P0 CapabilityGrant 闭环 ✅
### P1-A world_demo.vasm Vertical Execution Proof ✅
### P1-B 跨对象事务矩阵 + Grant 闭环审计 ✅

VASM-LIMITED 结论：
- VASM 有 CAPABILITY_GRANT 指令
- VASM 不支持跨 session（一次 module = 一个 transaction）
- 跨 session grant 主链已由 WorldService/veritasd 测试覆盖
- 不需要 VASM e2e demo

### 测试注释全覆盖 ✅
- meta_verification_comments.rs 强制所有测试有 //! 和 /// 注释
- 85 个缺失已补齐
- gen_verification_map.py 自动生成 VERIFICATION_MAP

## 待完成

### 可选增强（非阻塞）
A grant B on C -> commit -> WAL recovery -> 新 B session -> B 第一次依靠 recovered grant 操作 C -> 成功

### Stage 2（World State 完整性，world.md 12 主线）
1. WorldSnapshot 扩展为八组件
2. StateEntry 真实 version
3. Object Death 清理 StateStore
4. Checkpoint 保存/恢复完整 Machine State
5. Recovery 恢复计数器续接
6. Replay 统一走 TransactionDelta -> apply()
7. root_hash 升级 SHA-256

## 下一步：Forge Adapter

审查 Forge Intent -> capability/grant 语义 -> veritasd JSONL -> WorldService -> Kernel
特别是 Forge 的 agent/task 身份如何对应 grantee/grantor/resource/capability_id
