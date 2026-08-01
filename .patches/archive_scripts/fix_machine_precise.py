with open('src/machine.rs', 'r') as f:
    lines = f.readlines()

new_lines = []
for i, line in enumerate(lines):
    new_lines.append(line)
    # Line index: find "self.pc += consumed" that comes after execute_instruction
    if 'self.pc += consumed;' in line and i > 310 and i < 330:
        # Check if previous lines contain execute_instruction
        # Insert Commit reset before this line
        new_lines.insert(-1, '        if matches!(instruction, Instruction::Commit) {\n')
        new_lines.insert(-1, '            let current_object = self.ctx.current_object;\n')
        new_lines.insert(-1, '            self.ctx = self.engine.begin();\n')
        new_lines.insert(-1, '            self.ctx.current_object = current_object;\n')
        new_lines.insert(-1, '        }\n')
        break  # Only do this once

with open('src/machine.rs', 'w') as f:
    f.writelines(new_lines)

print('Done: precise machine.rs fix')
