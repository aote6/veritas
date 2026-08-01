with open('src/machine.rs', 'r') as f:
    content = f.read()

# Insert after Commit succeeds: begin new transaction, keep current_object
old = '''        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''
new = '''        let is_commit = matches!(instruction, Instruction::Commit);
        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''

content = content.replace(old, new)

# After successful execute_instruction, if Commit, reset ctx
old = '''        self.pc += consumed;'''
new = '''        if is_commit {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }

        self.pc += consumed;'''

content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: Machine now resets TransactionContext after Commit')
