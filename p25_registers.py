with open('src/machine.rs', 'r') as f:
    content = f.read()

# 1. Add registers field to CallFrame
old_struct = '''struct CallFrame {
    return_pc: usize,
    parent_object: ObjectId,
}'''

new_struct = '''struct CallFrame {
    return_pc: usize,
    parent_object: ObjectId,
    registers: RegisterFile,
}'''

content = content.replace(old_struct, new_struct)

# 2. Add registers save in Call
old_call = '''            Instruction::Call { object_id, entry_pc } => {
                let return_pc = self.pc + consumed;
                let saved_object = self.ctx.current_object;
                self.call_stack.push(CallFrame {
                    return_pc,
                    parent_object: saved_object,
                });'''

new_call = '''            Instruction::Call { object_id, entry_pc } => {
                let return_pc = self.pc + consumed;
                let saved_object = self.ctx.current_object;
                self.call_stack.push(CallFrame {
                    return_pc,
                    parent_object: saved_object,
                    registers: self.registers.clone(),
                });'''

content = content.replace(old_call, new_call)

# 3. Add registers restore in Return
old_return = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx.current_object = frame.parent_object;
                        self.pc = frame.return_pc;
                    }'''

new_return = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx.current_object = frame.parent_object;
                        self.registers = frame.registers;
                        self.pc = frame.return_pc;
                    }'''

content = content.replace(old_return, new_return)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
