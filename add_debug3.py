with open('src/engine.rs', 'r') as f:
    content = f.read()

# Add debug in commit
old = '''        self.detect_conflict(ctx)?;'''
new = '''        eprintln!("COMMIT tx_id={} current_object={} read_set_len={} write_set_len={}",
            ctx.tx_id(), ctx.current_object, ctx.read_set.states.len(), ctx.write_set.changes.len());
        self.detect_conflict(ctx)?;'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Debug added')
