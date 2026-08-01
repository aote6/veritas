with open('src/executor.rs', 'r') as f:
    content = f.read()

content = content.replace(
    'Instruction::LoadConst { .. } | Instruction::Add { .. }',
    'Instruction::HostCall { .. } | Instruction::LoadConst { .. } | Instruction::Add { .. }'
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done')
