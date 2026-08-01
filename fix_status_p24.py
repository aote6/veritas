with open('STATUS.md', 'r') as f:
    content = f.read()

old = '''- 测试：153 tests passing（新增test_call_switches_current_object_and_isolates_memory）
  导致engine.commit检查tx_id时失败（WriteConflict）
- 修复：CallFrame改为只保存return_pc+parent_object；Return调用
  engine.begin()创建新事务，current_object恢复为parent_object
- 问题：CallFrame保存了整个父ctx，Return时把已commit并移除的tx复活，
- 结果：153 tests passing，旧事务不再被复活
- 语义：Call=事务切换（新tx），Return=事务切换（新tx），
  caller/callee事务生命周期独立'''

new = '''- 测试：153 tests passing（新增test_call_switches_current_object_and_isolates_memory）

### P24.1 修复：CallFrame不再保存完整TransactionContext（2026-08-01）
- 问题：CallFrame保存了整个父ctx，Return时把已commit并移除的tx复活，
  导致engine.commit检查tx_id时失败（WriteConflict）
- 修复：CallFrame改为只保存return_pc+parent_object；Return调用
  engine.begin()创建新事务，current_object恢复为parent_object
- 结果：153 tests passing，旧事务不再被复活
- 语义：Call=事务切换（新tx），Return=事务切换（新tx），
  caller/callee事务生命周期独立'''

content = content.replace(old, new)

with open('STATUS.md', 'w') as f:
    f.write(content)

print('Done')
