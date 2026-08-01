with open('src/executor.rs', 'r') as f:
    content = f.read()

old = '''            Instruction::ObjectLink { from, to, relation } => {
                self.engine.object_link(ctx, *from, *to, *relation)?;
            }'''

new = '''            Instruction::ObjectLink { from, to, relation } => {
                self.engine.object_link(ctx, *from, *to, *relation)?;
            }
            Instruction::ObjectUnlink { from, to } => {
                self.engine.object_unlink(ctx, *from, *to)?;
            }'''

content = content.replace(old, new)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor.rs')
