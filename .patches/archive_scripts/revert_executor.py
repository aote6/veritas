with open('src/executor.rs', 'r') as f:
    content = f.read()

# Revert read_state
content = content.replace(
    'pub fn read_state(&self, ctx: &mut TransactionContext, addr: crate::types::Address) -> Result<Vec<u8>, VeritasError> {\n        self.engine.read(ctx, addr)',
    'pub fn read_state(&self, ctx: &mut TransactionContext, state_id: crate::types::StateId) -> Result<Vec<u8>, VeritasError> {\n        self.engine.read(ctx, state_id)'
)

# Revert write_state
content = content.replace(
    'pub fn write_state(&mut self, ctx: &mut TransactionContext, addr: crate::types::Address, payload: Vec<u8>) -> Result<(), VeritasError> {\n        self.engine.write(ctx, addr, payload)',
    'pub fn write_state(&mut self, ctx: &mut TransactionContext, state_id: crate::types::StateId, payload: Vec<u8>) -> Result<(), VeritasError> {\n        self.engine.write(ctx, state_id, payload)'
)

# Revert Instruction match arms
content = content.replace(
    '''Instruction::Read { state_id } => {
                let addr = crate::types::Address::new(ctx.current_object, *state_id);
                self.engine.read(ctx, addr)?;''',
    '''Instruction::Read { state_id } => {
                self.engine.read(ctx, *state_id)?;'''
)

content = content.replace(
    '''Instruction::Write { state_id, payload } => {
                let addr = crate::types::Address::new(ctx.current_object, *state_id);
                self.engine.write(ctx, addr, payload.clone())?;''',
    '''Instruction::Write { state_id, payload } => {
                self.engine.write(ctx, *state_id, payload.clone())?;'''
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor reverted to StateId')
