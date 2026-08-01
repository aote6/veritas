with open('src/engine.rs', 'r') as f:
    content = f.read()

# Fix line 101: Address::new(ctx.current_object, *state_id) -> *addr
content = content.replace(
    'mem.write(crate::types::Address::new(ctx.current_object, *state_id), payload.clone())',
    'mem.write(*addr, payload.clone())'
)

# Fix line 399: Address::new(ctx.current_object, *state_id) -> *addr
content = content.replace(
    'self.state_store.insert(\n                crate::types::Address::new(ctx.current_object, *state_id),',
    'self.state_store.insert(\n                *addr,'
)

# Fix line 637: Address::new(ctx.current_object, *state_id) -> *addr
content = content.replace(
    'if let Some(entry) = self.state_store.read(crate::types::Address::new(ctx.current_object, *state_id)) {',
    'if let Some(entry) = self.state_store.read(*addr) {'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: fixed three remaining state_id references')
