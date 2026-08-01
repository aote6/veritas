with open('src/engine.rs', 'r') as f:
    content = f.read()

# Line 69: if *state_id != info.resource
# This is in verify_capability, the loop variable is (addr, _)
# Change state_id to addr.state_id
content = content.replace(
    'if *state_id != info.resource {',
    'if addr.state_id != info.resource {'
)

# Line 101: in apply_state_memory, loop is (addr, payload)
# *state_id -> addr.state_id
content = content.replace(
    'self.state_store.write(crate::types::Address::new(ctx.current_object, *state_id), payload.clone())',
    'self.state_store.write(*addr, payload.clone())'
)

# Line 244: addr not found - need to check context
# This is likely in a place where addr needs to be constructed

# Line 352: writes_map.insert(*state_id, value.clone())
content = content.replace(
    'writes_map.insert(*state_id, value.clone())',
    'writes_map.insert(addr.state_id, value.clone())'
)

# Line 398: similar to 101
# Already fixed if same pattern

# Line 636: similar
# Already fixed if same pattern

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: remaining state_id -> addr.state_id')
