with open('src/machine.rs', 'r') as f:
    content = f.read()

# Fix LoadStateU64 and LoadStateBytes - need to construct addr before calling read_state
content = content.replace(
    'let bytes = self.executor.read_state(&mut self.ctx, state_id)?;',
    'let addr = crate::types::Address::new(self.ctx.current_object, state_id);\n                let bytes = self.executor.read_state(&mut self.ctx, addr)?;'
)

# But there are two occurrences. The first replacement will handle both.
# Check if there are remaining bare state_id usages

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: machine.rs read_state fixed')
