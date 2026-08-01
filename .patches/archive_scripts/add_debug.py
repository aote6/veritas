with open('src/engine.rs', 'r') as f:
    content = f.read()

# Add debug in commit write loop
old = '''        for (addr, value) in ctx.write_set.iter() {
            self.state_store.insert(
                *addr,
                StateEntry {
                    value: value.clone(),
                    version: commit_version,
                },
            );'''

new = '''        for (addr, value) in ctx.write_set.iter() {
            eprintln!("COMMIT WRITE object={} state={} value_len={}",
                addr.object_id, addr.state_id, value.len());
            self.state_store.insert(
                *addr,
                StateEntry {
                    value: value.clone(),
                    version: commit_version,
                },
            );'''

content = content.replace(old, new)

# Add debug in read
old = '''    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        if ctx.is_aborted() {'''

new = '''    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        let addr = crate::types::Address::new(ctx.current_object, state_id);
        eprintln!("READ object={} state={}", ctx.current_object, state_id);
        if ctx.is_aborted() {'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Debug added')
