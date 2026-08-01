with open('docs/constitution/transaction.md', 'r') as f:
    content = f.read()

old = '''| TransactionContext | types.rs TransactionContext | 增加 call_stack |
| ReadSet/WriteSet | engine.rs | 键改为 Address（P24 引入） |
| current_object | 散落在 machine.rs | 移到 TransactionContext |
| call_stack | 不存在 | 新增 |
| 跨 Object 事务 | 未明确定义 | 按此规范实现 |
| Snapshot | state_memory.rs | 保持 |
| Savepoint | engine.rs savepoint | 保持现状，未来扩展 |'''

new = '''| TransactionContext | types.rs TransactionContext | 已实现 |
| ReadSet/WriteSet | engine.rs | 键已改为 Address（P24） |
| current_object | machine.rs ctx.current_object | 属于 TransactionContext |
| call_stack | machine.rs Vec<CallFrame> | 已实现 |
| 跨 Object 事务 | machine.rs Call/Return | 同一 tx 内切换 current_object |
| Snapshot | state_memory.rs | 保持 |
| Savepoint | engine.rs savepoint | 保持现状，未来扩展 |'''

content = content.replace(old, new)

with open('docs/constitution/transaction.md', 'w') as f:
    f.write(content)

print('Done')
