with open('src/executor.rs', 'r') as f:
    content = f.read()

content = content.replace(
    'pub fn read_state(&self, ctx: &mut TransactionContext, state_id: crate::types::StateId) -> Result<Vec<u8>, VeritasError> {\n        self.engine.read(ctx, state_id)',
    'pub fn read_state(&self, ctx: &mut TransactionContext, addr: crate::types::Address) -> Result<Vec<u8>, VeritasError> {\n        self.engine.read(ctx, addr)'
)

content = content.replace(
    'pub fn write_state(&mut self, ctx: &mut TransactionContext, state_id: crate::types::StateId, payload: Vec<u8>) -> Result<(), VeritasError> {\n        self.engine.write(ctx, state_id, payload)',
    'pub fn write_state(&mut self, ctx: &mut TransactionContext, addr: crate::types::Address, payload: Vec<u8>) -> Result<(), VeritasError> {\n        self.engine.write(ctx, addr, payload)'
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor.rs fixed')
