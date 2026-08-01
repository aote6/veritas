with open('src/executor.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '''Instruction::Read { state_id } => {
                self.engine.read(ctx, *state_id)?;''',
    '''Instruction::Read { state_id } => {
                let addr = crate::types::Address::new(ctx.current_object, *state_id);
                self.engine.read(ctx, addr)?;'''
)

content = content.replace(
    '''Instruction::Write { state_id, payload } => {
                self.engine.write(ctx, *state_id, payload.clone())?;''',
    '''Instruction::Write { state_id, payload } => {
                let addr = crate::types::Address::new(ctx.current_object, *state_id);
                self.engine.write(ctx, addr, payload.clone())?;'''
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor.rs Instruction matches fixed')
