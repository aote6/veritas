with open('src/executor.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '''            Instruction::ObjectDeath { object_id } => {
                self.engine.object_death(ctx, *object_id)?;
            }''',
    '''            Instruction::ObjectDeath { object_id } => {
                self.engine.object_death(ctx, *object_id)?;
            }
            Instruction::ObjectFreeze { object_id } => {
                self.engine.object_freeze(ctx, *object_id)?;
            }'''
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor.rs')
