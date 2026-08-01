with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''        {
            let mut topo = self.topology.lock().unwrap();
            for edge in &ctx.pending_links {
                topo.push(edge.clone());
            }
        }'''

new = '''        {
            let mut topo = self.topology.lock().unwrap();
            for edge in &ctx.pending_links {
                topo.push(edge.clone());
            }
            // P26: 处理 pending_unlinks
            for (from, to) in &ctx.pending_unlinks {
                topo.retain(|e| e.from != *from || e.to != *to);
            }
        }'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
