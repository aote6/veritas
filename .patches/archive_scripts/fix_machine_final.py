with open('src/machine.rs', 'r') as f:
    content = f.read()

# 1. Remove duplicate Commit reset block
old_dup = '''        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }
        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }'''

new_single = '''        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }'''

content = content.replace(old_dup, new_single)

# 2. Add call_stack field to Machine struct
old_struct = '    pub execution: crate::execution::ExecutionContext,'
new_struct = '''    pub execution: crate::execution::ExecutionContext,
    call_stack: Vec<(usize, ObjectId)>,  // (return_pc, saved_object)'''

content = content.replace(old_struct, new_struct)

# 3. Add call_stack init in Machine::new
old_init = '            execution: crate::execution::ExecutionContext::new(),'
new_init = '''            execution: crate::execution::ExecutionContext::new(),
            call_stack: Vec::new(),'''

content = content.replace(old_init, new_init)

# 4. Add Call/Return branches in step() match
# Find Jn branch and insert Call/Return after it
old_jn = '''            Instruction::Jn { target } => {
                if self.flags.negative { self.pc = target; } else { self.pc += consumed; }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            _ => {}'''

new_call_return = '''            Instruction::Jn { target } => {
                if self.flags.negative { self.pc = target; } else { self.pc += consumed; }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::Call { object_id, entry_pc } => {
                let return_pc = self.pc + consumed;
                let saved_object = self.ctx.current_object;
                self.call_stack.push((return_pc, saved_object));
                self.ctx.current_object = object_id;
                self.pc = entry_pc;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::Return => {
                match self.call_stack.pop() {
                    Some((return_pc, saved_object)) => {
                        self.pc = return_pc;
                        self.ctx.current_object = saved_object;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            _ => {}'''

content = content.replace(old_jn, new_call_return)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: Call/Return implemented, duplicate removed')
