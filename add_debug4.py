with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''    pub fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) -> Result<(), VeritasError> {
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        if ctx.is_aborted() {'''

new = '''    pub fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) -> Result<(), VeritasError> {
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        eprintln!("WRITE tx_id={} object={} state={}", ctx.tx_id(), ctx.current_object, state_id);
        if ctx.is_aborted() {'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Debug added')
