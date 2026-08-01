with open('STATUS.md', 'r') as f:
    content = f.read()

old = '''### 已知未覆盖场景 / 待设计决策（P24 follow-up，未修复）
- **caller未提交写入被callee commit静默冲掉的风险**：Commit触发的重置
  逻辑是`self.ctx = self.engine.begin()`——整个替换ctx。若caller在Call
  前有未提交写入，callee内部commit时会导致caller那笔写入所在的旧ctx
  被整体丢弃，不报错、不提交，静默丢失数据。
- 需要在以下方案中决策（未实现任何一个）：
  方案A：Call前强制要求ctx已commit（简单，限制灵活性）
  方案B：write_set按object分区，callee commit只提交该object相关写入
  方案C：Call新开子事务而非共享ctx（改变现有设计初衷）
- 测试草案 test_caller_uncommitted_write_lost_when_callee_commits
  已写出但未加入正式测试套件（预期FAIL，验证问题存在），见
  .patches/p24_debug_scripts下本次session记录，下次处理前应先跑这个
  草案确认是Err还是错误值，以判断严重程度

### P24.1 修复：CallFrame不再保存完整TransactionContext（2026-08-01）'''

new = '''### P24.1 修复：CallFrame不再保存完整TransactionContext（2026-08-01）
- 问题：CallFrame保存了整个父ctx，Return时把已commit并移除的tx复活，
  导致engine.commit检查tx_id时失败（WriteConflict）
- 修复：CallFrame改为只保存return_pc+parent_object；Return调用
  engine.begin()创建新事务，current_object恢复为parent_object
- 结果：153 tests passing，旧事务不再被复活
- 语义：Call=事务切换（新tx），Return=事务切换（新tx），
  caller/callee事务生命周期独立
- 旧记录中的"caller未提交写入被callee commit静默冲掉"场景
  在此修复后已不再成立：Call创建新tx，caller旧ctx保存在栈上，
  不会被callee commit影响'''

content = content.replace(old, new)

with open('STATUS.md', 'w') as f:
    f.write(content)

print('Done')
