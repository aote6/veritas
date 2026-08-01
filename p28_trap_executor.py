with open('src/executor.rs', 'r') as f:
    content = f.read()

content = content.replace(
    'Instruction::HostCall { .. } | Instruction::LoadConst { .. }',
    'Instruction::Trap { .. } | Instruction::HostCall { .. } | Instruction::LoadConst { .. }'
)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done: executor.rs')
