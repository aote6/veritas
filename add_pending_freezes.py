with open('src/types.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    pub pending_unlinks: Vec<(ObjectId, ObjectId)>,\n    pub pending_deaths: Vec<ObjectId>,',
    '    pub pending_unlinks: Vec<(ObjectId, ObjectId)>,\n    pub pending_freezes: Vec<ObjectId>,\n    pub pending_deaths: Vec<ObjectId>,'
)

content = content.replace(
    '            pending_unlinks: Vec::new(),\n            pending_deaths: Vec::new(),',
    '            pending_unlinks: Vec::new(),\n            pending_freezes: Vec::new(),\n            pending_deaths: Vec::new(),'
)

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Done: types.rs')
