with open('src/types.rs', 'r') as f:
    content = f.read()

content = content.replace(
    'capability_enforced: false,',
    'capability_enforced: true,'
)

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Default capability_enforced = true')
