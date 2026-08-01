with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''        let call_inst = Instruction::Call { object_id: 2, entry_pc: caller_len + Instruction::Call{object_id:2, entry_pc:0}.encode().unwrap().len() };
        let call_len = call_inst.encode().unwrap().len();
        let callee_entry_pc = caller_len + call_len;

        let mut full = caller_program.instructions.clone();
        full.push(Instruction::Call { object_id: 2, entry_pc: callee_entry_pc });
        full.push(Instruction::LoadConst { reg: 1, val: 999 });
        full.push(Instruction::WriteRegister { state_id: 1, reg: 1 });
        full.push(Instruction::Commit);
        full.push(Instruction::Halt);
        for inst in &callee_program.instructions {
            full.push(inst.clone());
        }'''

new = '''        // 先算出Call指令本身的编码长度（用占位entry_pc=0，因为Call是定长编码，
        // entry_pc数值大小不影响编码长度，这个假设需要成立，否则要用最终值重算）
        let call_len = Instruction::Call { object_id: 2, entry_pc: 0 }.encode().unwrap().len();

        // caller调用后的收尾指令（写999并commit，然后halt）
        let after_call_instructions = vec![
            Instruction::LoadConst { reg: 1, val: 999 },
            Instruction::WriteRegister { state_id: 1, reg: 1 },
            Instruction::Commit,
            Instruction::Halt,
        ];
        let after_call_len: usize = after_call_instructions.iter()
            .map(|i| i.encode().unwrap().len())
            .sum();

        // callee真正的入口 = caller长度 + Call指令长度 + 收尾指令长度
        let callee_entry_pc = caller_len + call_len + after_call_len;

        let mut full = caller_program.instructions.clone();
        full.push(Instruction::Call { object_id: 2, entry_pc: callee_entry_pc });
        for inst in &after_call_instructions {
            full.push(inst.clone());
        }
        for inst in &callee_program.instructions {
            full.push(inst.clone());
        }'''

assert old in content, "old block not found — check current test code"
content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print("Fixed entry_pc calculation in test")
