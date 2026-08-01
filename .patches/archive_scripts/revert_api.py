with open('src/engine.rs', 'r') as f:
    content = f.read()

# Revert read signature back to state_id: StateId
content = content.replace(
    '    pub fn read(\n        &self,\n        ctx: &mut TransactionContext,\n        addr: Address,\n    ) -> Result<Vec<u8>, VeritasError> {',
    '    pub fn read(\n        &self,\n        ctx: &mut TransactionContext,\n        state_id: StateId,\n    ) -> Result<Vec<u8>, VeritasError> {\n        let addr = crate::types::Address::new(ctx.current_object, state_id);'
)

# Revert write signature back to state_id: StateId
content = content.replace(
    '    pub fn write(\n        &self,\n        ctx: &mut TransactionContext,\n        addr: Address,\n        value: Vec<u8>,\n    ) -> Result<(), VeritasError> {',
    '    pub fn write(\n        &self,\n        ctx: &mut TransactionContext,\n        state_id: StateId,\n        value: Vec<u8>,\n    ) -> Result<(), VeritasError> {\n        let addr = crate::types::Address::new(ctx.current_object, state_id);'
)

# Fix init_state_in_tx back
content = content.replace(
    '    pub fn init_state_in_tx(\n        &self,\n        ctx: &mut TransactionContext,\n        addr: Address,\n        value: Vec<u8>,\n    ) {',
    '    pub fn init_state_in_tx(\n        &self,\n        ctx: &mut TransactionContext,\n        state_id: StateId,\n        value: Vec<u8>,\n    ) {\n        let addr = crate::types::Address::new(ctx.current_object, state_id);'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine API reverted to StateId')
