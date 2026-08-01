with open('/data/data/com.termux/files/home/veritas_kernel/STATUS.md', 'r') as f:
    content = f.read()

anchor = "  详见commit 2f3f656。\n"

p24_section = """
### Call/Return跨Object调用（P24，2026-08-01）
- 修复：`Instruction::Call`/`Instruction::Return`此前在`Machine::step()`
  中完全未被处理，全部落入`_ => {}`兜底，是纯粹的未实现功能（不是回归）
- 实现：`Machine`新增`call_stack: Vec<(usize, ObjectId)>`字段，
  `Call{object_id, entry_pc}`压栈保存(return_pc, saved_current_object)、
  切换current_object、跳转pc；`Return`弹栈恢复。当前设计为
  caller/callee共享同一TransactionContext，不新开事务
  （instruction.rs注释明确：暂不涉及独立代码空间）
- 测试：153 tests passing（新增test_call_switches_current_object_and_isolates_memory）

### 已知未覆盖场景 / 待设计决策（P24 follow-up，未修复）
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

"""

assert anchor in content, "anchor text not found, check STATUS.md line 46 exactly"
content = content.replace(anchor, anchor + p24_section)

with open('/data/data/com.termux/files/home/veritas_kernel/STATUS.md', 'w') as f:
    f.write(content)

print("P24 section inserted into STATUS.md")
