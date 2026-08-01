with open('src/machine.rs', 'r') as f:
    content = f.read()

old = '''        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }

'''

# Check if already present
if 'if matches!(instruction, Instruction::Commit)' not in content:
    # Insert before self.pc += consumed
    old_target = '''        self.pc += consumed;'''
    new_target = '''        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }

        self.pc += consumed;'''
    content = content.replace(old_target, new_target)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
