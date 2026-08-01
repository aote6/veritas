with open('src/machine.rs', 'r') as f:
    content = f.read()

# 1. Replace Call branch
old_call = '''            Instruction::Call { object_id, entry_pc } => {
                let return_pc = self.pc + consumed;
                let saved_object = self.ctx.current_object;
                self.call_stack.push((return_pc, saved_object));
                self.ctx.current_object = object_id;
                self.pc = entry_pc;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }'''

new_call = '''            Instruction::Call { object_id, entry_pc } => {
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

content = content.replace(old_call, new_call)

# 2. Replace Return branch
old_return = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some((return_pc, saved_object)) => {
                        self.pc = return_pc;
                        self.ctx.current_object = saved_object;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }'''

new_return = '''            Instruction::Return => {
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

content = content.replace(old_return, new_return)

# 3. Delete Commit reset block
old_commit_reset = '''        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }

'''

content = content.replace(old_commit_reset, '')

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
