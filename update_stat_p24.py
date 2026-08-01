with open('STATUS.md', 'r') as f:
    content = f.read()

old = '''### P24 已知隐患（未修复，2026-08-01）
- callee commit 可能冲掉 caller 未提交写入：caller 在 Call 前有未提交
  的 write_set，Call 在同一 tx 内切换 current_object，callee 执行 Commit
  后 tx 被重置（engine.begin()），caller 的未提交写入随旧 ctx 被丢弃
- 触发条件：caller 未提交写入 + Call + callee commit。如果 caller 先
  commit 再 Call 则不受影响，当前 155 个测试全部走的是这条安全路径
- 待设计方案（三选一）：
  方案A：Call 前强制 caller 已 commit，否则报错
  方案B：write_set 按 Object 分区，callee commit 只提交自己的写入
  方案C：Call 时自动 commit caller 的未提交写入（隐式事务边界）
- 当前未修复，未选择方案，作为已知限制保留'''

new = '''### P24 隐患修复：禁止嵌套 Commit（2026-08-01）
- 根因：callee commit 不是"丢弃"caller 写入，而是把 caller 未提交的
  写入**提前持久化**。caller 和 callee 共享同一 TransactionContext，
  callee commit 将整个 write_set（含 caller 部分）原子落盘，caller
  的事务被静默拆成两半
- 修复：Machine 层检查 call_stack，非空时拒绝 Commit 并 Abort。
  依据宪法 transaction.md：Transaction 不可嵌套，CALL/RETURN 不改变
  Transaction 边界，Commit 只能在最外层执行
- P24 测试已更新：callee 不再 Commit，由 caller 在最外层统一提交
- 155 tests passing'''

content = content.replace(old, new)

with open('STATUS.md', 'w') as f:
    f.write(content)

print('Done')
