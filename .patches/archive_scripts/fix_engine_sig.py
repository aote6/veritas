with open('src/engine.rs', 'r') as f:
    content = f.read()

# Change read signature: state_id: StateId -> addr: Address
content = content.replace(
    '    pub fn read(\n        &self,\n        ctx: &mut TransactionContext,\n        state_id: StateId,\n    ) -> Result<Vec<u8>, VeritasError> {\n        let addr = crate::types::Address::new(ctx.current_object, state_id);',
    '    pub fn read(\n        &self,\n        ctx: &mut TransactionContext,\n        addr: Address,\n    ) -> Result<Vec<u8>, VeritasError> {'
)

# Change write signature: state_id: StateId -> addr: Address
content = content.replace(
    '    pub fn write(\n        &self,\n        ctx: &mut TransactionContext,\n        state_id: StateId,\n        value: Vec<u8>,\n    ) -> Result<(), VeritasError> {\n        let addr = crate::types::Address::new(ctx.current_object, state_id);',
    '    pub fn write(\n        &self,\n        ctx: &mut TransactionContext,\n        addr: Address,\n        value: Vec<u8>,\n    ) -> Result<(), VeritasError> {'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine signatures changed to Address')
