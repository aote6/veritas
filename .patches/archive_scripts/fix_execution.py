with open('src/execution.rs', 'r') as f:
    content = f.read()

# Fix import
content = content.replace(
    'use crate::types::{WriteSet, StateId};',
    'use crate::types::{WriteSet, StateId, Address};'
)

# Fix record_write signature
content = content.replace(
    'pub fn record_write(&mut self, state_id: StateId, value: Vec<u8>)',
    'pub fn record_write(&mut self, addr: Address, value: Vec<u8>)'
)

# Fix record_write body - push
content = content.replace(
    'self.writes.push(state_id, value);',
    'self.writes.push(addr, value);'
)

# Fix the event recording - need to extract state_id from addr
content = content.replace(
    'self.events.push(ExecutionEvent::StateWrite { state_id, len: value.len() });',
    'self.events.push(ExecutionEvent::StateWrite { state_id: addr.state_id, len: value.len() });'
)

with open('src/execution.rs', 'w') as f:
    f.write(content)

print('Done: execution.rs fixed')
