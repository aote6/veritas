with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''        let callee_program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 200 })
            .push(Instruction::WriteRegister { state_id: 1, reg: 0 })
            .push(Instruction::Commit)
            .push(Instruction::Return);'''

new = '''        // callee只写数据然后Return，不在嵌套调用内Commit
        // 宪法transaction.md：Transaction不可嵌套，Commit只能在最外层
        let callee_program = Program::new()
            .push(Instruction::LoadConst { reg: 0, val: 200 })
            .push(Instruction::WriteRegister { state_id: 1, reg: 0 })
            .push(Instruction::Return);'''

content = content.replace(old, new)

# callee_len需要重算
# entry_pc也需要重算，因为callee少了一条指令
# 先替换代码再手动修数字——不，应该保留entry_pc计算，因为callee_len会自动更新

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
