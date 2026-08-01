with open('src/engine.rs', 'r') as f:
    content = f.read()

old = 'hist.push(ReplayRecord::new(ctx.tx_id(), ctx.capability_id, ctx.program_hash.unwrap_or(0), write_set.changes.clone(), before, after));'
new = '''let writes_for_record: Vec<(crate::types::StateId, Vec<u8>)> = write_set.changes.iter().map(|(addr, val)| (addr.state_id, val.clone())).collect();
        hist.push(ReplayRecord::new(ctx.tx_id(), ctx.capability_id, ctx.program_hash.unwrap_or(0), writes_for_record, before, after));'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: ReplayRecord conversion added')
