with open('src/machine.rs', 'r') as f:
    content = f.read()

# 1. Simplify CallFrame
old_struct = '''#[derive(Debug)]
struct CallFrame {
    ctx: TransactionContext,
    return_pc: usize,
}'''

new_struct = '''#[derive(Debug)]
struct CallFrame {
    return_pc: usize,
    parent_object: ObjectId,
}'''

content = content.replace(old_struct, new_struct)

# 2. Fix Call branch
old_call = '''            Instruction::Call { object_id, entry_pc } => {
                let return_pc = self.pc + consumed;
                let parent_ctx = std::mem::replace(
                    &mut self.ctx,
                    self.engine.begin(),
                );
                self.call_stack.push(CallFrame {
                    ctx: parent_ctx,
                    return_pc,
                });
                self.ctx.current_object = object_id;
                self.pc = entry_pc;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }'''

new_call = '''            Instruction::Call { object_id, entry_pc } => {
                let frame = CallFrame {
                    return_pc: self.pc + consumed,
                    parent_object: self.ctx.current_object,
                };
                self.call_stack.push(frame);
                self.ctx = self.engine.begin();
                self.ctx.current_object = object_id;
                self.pc = entry_pc;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }'''

content = content.replace(old_call, new_call)

# 3. Fix Return branch
old_return = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx = frame.ctx;
                        self.pc = frame.return_pc;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }'''

new_return = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx = self.engine.begin();
                        self.ctx.current_object = frame.parent_object;
                        self.pc = frame.return_pc;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }'''

content = content.replace(old_return, new_return)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
