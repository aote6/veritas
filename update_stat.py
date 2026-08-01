with open('STATUS.md', 'r') as f:
    content = f.read()

old = '''### P24.1 修复：CallFrame不再保存完整TransactionContext（2026-08-01）
- 问题：CallFrame保存了整个父ctx，Return时把已commit并移除的tx复活，
  导致engine.commit检查tx_id时失败（WriteConflict）
- 修复：CallFrame改为只保存return_pc+parent_object；Return调用
  engine.begin()创建新事务，current_object恢复为parent_object
- 结果：153 tests passing，旧事务不再被复活
- 语义：Call=事务切换（新tx），Return=事务切换（新tx），
  caller/callee事务生命周期独立'''

new = '''### P24 最终方案：Call/Return在同一事务内切换current_object（2026-08-01）
- Call：保存return_pc+parent_object到call_stack，切换current_object，
  不创建新事务。caller和callee共享同一TransactionContext
- Return：从call_stack恢复current_object和pc，不创建新事务
- Commit：成功后重置TransactionContext（保留current_object），
  新事务拥有新snapshot和空read_set/write_set
- 删除P24.1的"Call/Return无条件创建新tx"方案——该方案导致
  caller在Call前未提交写入被无条件丢弃，比原始bug更严重
- 153 tests passing'''

content = content.replace(old, new)

with open('STATUS.md', 'w') as f:
    f.write(content)

print('Done')
