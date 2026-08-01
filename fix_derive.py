with open('src/machine.rs', 'r') as f:
    content = f.read()

# Remove wrongly placed derive and add it correctly
content = content.replace(
    '#[derive(Debug, Clone)]\npub struct RegisterFile {',
    'pub struct RegisterFile {'
)
content = content.replace(
    'pub struct RegisterFile {',
    '#[derive(Debug, Clone)]\npub struct RegisterFile {'
)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
