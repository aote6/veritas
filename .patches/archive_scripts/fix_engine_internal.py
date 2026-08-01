with open('src/engine.rs', 'r') as f:
    content = f.read()

# In read(): remove the old Address::new call (already removed by previous replacement)
# But now state_id is used where addr should be used

# Line 287: Address::new(ctx.current_object, state_id) -> addr
content = content.replace(
    'crate::types::Address::new(ctx.current_object, state_id)',
    'addr'
)

# Line 290: state_id in error message
content = content.replace(
    '"State {:?} not found",\n                state_id',
    '"State {:?} not found",\n                addr.state_id'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine internal state_id -> addr')
