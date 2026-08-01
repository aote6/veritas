import re

with open('src/types.rs', 'r') as f:
    content = f.read()

# Fix ReadSet
content = content.replace(
    'pub states: HashMap<StateId, Version>,',
    'pub states: HashMap<Address, Version>,'
)

# Fix WriteSet changes
content = content.replace(
    'pub changes: Vec<(StateId, Vec<u8>)>,',
    'pub changes: Vec<(Address, Vec<u8>)>,'
)

# Fix WriteSet push signature
content = content.replace(
    'pub fn push(&mut self, state_id: StateId, value: Vec<u8>) {',
    'pub fn push(&mut self, addr: Address, value: Vec<u8>) {'
)

# Fix WriteSet push body
content = content.replace(
    'self.changes.push((state_id, value));',
    'self.changes.push((addr, value));'
)

# Fix WriteSet hash - change id type from StateId to Address
# The hash function uses id directly, need to change to use addr.state_id or addr.object_id
# For now, keep the hash logic but use addr.state_id for backward compat
content = content.replace(
    'for (id, data) in &self.changes {',
    'for (addr, data) in &self.changes {'
)
content = content.replace(
    'h ^= id;',
    'h ^= addr.state_id;'
)

# Fix get_latest signature
content = content.replace(
    'pub fn get_latest(&self, state_id: StateId) -> Option<&Vec<u8>>',
    'pub fn get_latest(&self, addr: Address) -> Option<&Vec<u8>>'
)

# Fix get_latest body
content = content.replace(
    '.find(|(id, _)| *id == state_id)',
    '.find(|(a, _)| *a == addr)'
)

# Fix contains_key signature
content = content.replace(
    'pub fn contains_key(&self, state_id: StateId) -> bool',
    'pub fn contains_key(&self, addr: Address) -> bool'
)

# Fix contains_key body
content = content.replace(
    'self.changes.iter().any(|(id, _)| *id == state_id)',
    'self.changes.iter().any(|(a, _)| *a == addr)'
)

# Fix keys return type
content = content.replace(
    'pub fn keys(&self) -> Vec<StateId>',
    'pub fn keys(&self) -> Vec<Address>'
)

# Fix keys body - id is now Address
content = content.replace(
    'let mut result = Vec::new();\n        for (id, _) in &self.changes {\n            if !seen.contains(id) {\n                seen.insert(*id);\n                result.push(*id);',
    'let mut result = Vec::new();\n        for (addr, _) in &self.changes {\n            if !seen.contains(addr) {\n                seen.insert(*addr);\n                result.push(*addr);'
)

# Fix iter return type
content = content.replace(
    "pub fn iter(&self) -> std::slice::Iter<'_, (StateId, Vec<u8>)>",
    "pub fn iter(&self) -> std::slice::Iter<'_, (Address, Vec<u8>)>"
)

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Done: types.rs updated')
