with open('src/engine.rs', 'r') as f:
    lines = f.readlines()

# Find line numbers where addr needs to be inserted
# Insert after the opening brace of read() and write() methods
# read() starts at line 271 (0-indexed: 270), opening brace at line 275 (0-indexed: 274)
# write() starts at line 300 (0-indexed: 299), opening brace at line 304 (0-indexed: 303)

# Let's find them by content
new_lines = []
i = 0
while i < len(lines):
    new_lines.append(lines[i])
    
    # Detect read method opening brace
    if 'pub fn read(' in lines[i]:
        # Find the opening brace
        while i < len(lines) and '{' not in lines[i]:
            i += 1
            new_lines.append(lines[i])
        # Now lines[i] contains {
        # Insert addr after this line
        new_lines.append('        let addr = crate::types::Address::new(ctx.current_object, state_id);\n')
    
    # Detect write method opening brace
    elif 'pub fn write(' in lines[i]:
        while i < len(lines) and '{' not in lines[i]:
            i += 1
            new_lines.append(lines[i])
        new_lines.append('        let addr = crate::types::Address::new(ctx.current_object, state_id);\n')
    
    i += 1

with open('src/engine.rs', 'w') as f:
    f.writelines(new_lines)

print('Done: addr inserted in read() and write()')
