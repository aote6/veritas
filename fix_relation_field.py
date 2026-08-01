with open('src/engine.rs', 'r') as f:
    content = f.read()

# Fix line 151: relation -> link_type
content = content.replace(
    'recovered_links.push(LinkEdge { from: *from, to: *to, relation });',
    'recovered_links.push(LinkEdge { from: *from, to: *to, link_type: relation });'
)

# Fix line 601: relation -> link_type
content = content.replace(
    'ctx.pending_links.push(LinkEdge { from, to, relation });',
    'ctx.pending_links.push(LinkEdge { from, to, link_type: relation });'
)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
