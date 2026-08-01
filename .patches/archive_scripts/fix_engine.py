with open('src/engine.rs', 'r') as f:
    content = f.read()

# Fix read() - line 280 area: write_set.get_latest(state_id) -> write_set.get_latest(addr)
# But we need to add addr construction first. Let's handle the patterns.

# Pattern 1: ctx.write_set.push(state_id, value) -> ctx.write_set.push(addr, value)
# Pattern 2: ctx.write_set.get_latest(state_id) -> ctx.write_set.get_latest(addr)
# Pattern 3: ctx.write_set.contains_key(&state_id) -> ctx.write_set.contains_key(&addr)
# Pattern 4: ctx.read_set.states.insert(state_id, ...) -> ctx.read_set.states.insert(addr, ...)
# Pattern 5: ctx.read_set.states.contains_key(&state_id) -> ctx.read_set.states.contains_key(&addr)

# Pattern 6: for (state_id, ...) in &ctx.write_set -> for (addr, ...) in &ctx.write_set
# Pattern 7: for (state_id, ...) in &ctx.read_set.states -> for (addr, ...) in &ctx.read_set.states

# Fix WriteSet push calls
content = content.replace(
    'ctx.write_set.push(state_id, value)',
    'ctx.write_set.push(addr, value)'
)
content = content.replace(
    'ctx.write_set.push(state_id, payload)',
    'ctx.write_set.push(addr, payload)'
)
content = content.replace(
    'ctx.write_set.push(state_id, val)',
    'ctx.write_set.push(addr, val)'
)

# Fix WriteSet get_latest
content = content.replace(
    'ctx.write_set.get_latest(state_id)',
    'ctx.write_set.get_latest(addr)'
)

# Fix WriteSet contains_key
content = content.replace(
    'ctx.write_set.changes.is_empty()',
    'ctx.write_set.changes.is_empty()'
)

# Fix read_set.states.insert
content = content.replace(
    'ctx.read_set.states.insert(state_id, entry.version)',
    'ctx.read_set.states.insert(addr, entry.version)'
)

# Fix read_set.states.contains_key
content = content.replace(
    'ctx.read_set.states.contains_key(&state_id)',
    'ctx.read_set.states.contains_key(&addr)'
)

# Fix for loops over write_set.changes
content = content.replace(
    'for (state_id, value) in ctx.write_set.iter()',
    'for (addr, value) in ctx.write_set.iter()'
)
content = content.replace(
    'for (state_id, payload) in &write_set.changes',
    'for (addr, payload) in &write_set.changes'
)
content = content.replace(
    'for (state_id, value) in &ctx.write_set.changes',
    'for (addr, value) in &ctx.write_set.changes'
)
content = content.replace(
    'for (state_id, read_version) in &ctx.read_set.states',
    'for (addr, read_version) in &ctx.read_set.states'
)

# Fix detect_conflict - access memory with address
content = content.replace(
    'memory.version(state_id)',
    'memory.version(addr)'
)

# Fix apply_state_memory
content = content.replace(
    'for (state_id, payload) in &write_set.changes',
    'for (addr, payload) in &write_set.changes'
)

# Fix the write_set push inside engine.rs write method (line 316 area)
# Need to find the pattern and add addr construction before it

# Fix the verify_capability loop
content = content.replace(
    'for (state_id, _) in &ctx.write_set.changes',
    'for (addr, _) in &ctx.write_set.changes'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine.rs basic replacements')
