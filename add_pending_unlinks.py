with open('src/types.rs', 'r') as f:
    content = f.read()

# Add field to struct
content = content.replace(
    '    pub pending_links: Vec<LinkEdge>,\n    pub pending_deaths: Vec<ObjectId>,',
    '    pub pending_links: Vec<LinkEdge>,\n    pub pending_unlinks: Vec<(ObjectId, ObjectId)>,\n    pub pending_deaths: Vec<ObjectId>,'
)

# Add init in new()
content = content.replace(
    '            pending_links: Vec::new(),\n            pending_deaths: Vec::new(),',
    '            pending_links: Vec::new(),\n            pending_unlinks: Vec::new(),\n            pending_deaths: Vec::new(),'
)

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Done')
