with open('src/execution.rs', 'r') as f:
    content = f.read()

# Revert record_write to accept state_id
content = content.replace(
    'pub fn record_write(&mut self, addr: Address, value: Vec<u8>)',
    'pub fn record_write(&mut self, state_id: StateId, value: Vec<u8>)'
)

content = content.replace(
    'self.writes.push(addr, value);',
    'let addr = crate::types::Address::new(0, state_id);\n        self.writes.push(addr, value);'
)

content = content.replace(
    'self.events.push(ExecutionEvent::StateWrite { state_id: addr.state_id, len: value.len() });',
    'self.events.push(ExecutionEvent::StateWrite { state_id, len: value.len() });'
)

with open('src/execution.rs', 'w') as f:
    f.write(content)

print('Done: execution reverted to StateId')
