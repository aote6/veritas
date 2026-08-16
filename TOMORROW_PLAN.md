# 明日工作：打通 Forge ↔ Veritas 主工作流

日期：2026-08-16
状态：待执行

## 背景

Veritas 内核已冻结（Checkpoint Integrity + Replay Continuity）。
Forge 是 Veritas 上第一个程序，但当前主工作流没有真正使用 Veritas 世界。

现状：
- Forge 启动时能恢复身份、查 world_info ✅
- forge/world/ 里有完整的世界操作接口 ✅
- Planner 和主循环仍然直接操作文件系统 ❌
- "文件即 Object" 的统一视图没有形成 ❌

## 目标

让 Forge 的每次文件修改都通过 Veritas 事务完成。

理想闭环：

  用户任务
    → Planner 生成 Intent
    → IntentExecutor
    → WorldSession.create_object / write / commit
    → veritasd
    → Veritas Kernel
    → WAL + root_hash 变化
    → Receipt
    → Projection 读回文件内容
    → 用户看到结果

## 需要做的事

1. 审计 forge/intents/executor.py 的完整执行链
   - Intent 从哪里来
   - 哪些 Intent 走了 WorldSession
   - 哪些 Intent 还在直接操作文件系统

2. 审计 forge/world/session.py 的世界操作
   - create_object / write / commit 是否正确
   - commit 后有没有 Receipt 投影回文件

3. 审计 Projection 层
   - Object → 文件 的映射是否存在
   - 修改后有没有从 Veritas 读回文件内容

4. 打通主循环
   - 让 Planner 的修改意图真正路由到 WorldSession
   - 让 commit 后的 Receipt 触发文件投影
   - 让用户看到"修改已提交，root_hash 已变化"

5. 测试
   - 用一个小任务验证：创建文件 → 修改文件 → 删除文件
   - 每一步都确认 Veritas 世界的 object_count / root_hash 变化
   - 确认文件系统内容与 Veritas State 一致

## 不要做的事

- 不要改 Veritas 内核
- 不要改已冻结的 Checkpoint / Replay
- 不要给 attach_identity 加认证（WRI v2 再处理）
- 不要重构 Forge 的 Planner

## 验收标准

跑一次真实的文件修改任务，确认：
- Veritas WAL 里有对应的 ObjectBirth / Write / Commit 记录
- root_hash 在修改前后发生变化
- 文件内容与 Veritas State 一致
- Receipt 能证明这次修改

## 相关文件

- ~/forge/forge/intents/executor.py
- ~/forge/forge/world/session.py
- ~/forge/forge/world/runtime.py
- ~/forge/forge/world/adapter.py
- ~/veritas_kernel/src/bin/veritasd.rs
- ~/veritas_kernel/src/world_api.rs
