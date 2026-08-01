with open('src/engine.rs', 'r') as f:
    content = f.read()

# Add debug in detect_conflict
old = '''    fn detect_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        for (addr, read_version) in &ctx.read_set.states {
            if let Some(entry) = self.state_store.read(*addr) {
                if entry.version > *read_version {
                    return Err(AbortReason::WriteConflict);
                }
            }
        }'''

new = '''    fn detect_conflict(&self, ctx: &TransactionContext) -> Result<(), AbortReason> {
        for (addr, read_version) in &ctx.read_set.states {
            if let Some(entry) = self.state_store.read(*addr) {
                eprintln!("DETECT_CONFLICT addr=({},{}) entry.version={} read_version={}",
                    addr.object_id, addr.state_id, entry.version, read_version);
                if entry.version > *read_version {
                    return Err(AbortReason::WriteConflict);
                }
            }
        }'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Debug added')
