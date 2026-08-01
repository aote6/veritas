# Step 1: instruction.rs - add Trap variant
with open('src/instruction.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    HostCall,\n    ObjectFreeze,',
    '    Trap,\n    HostCall,\n    ObjectFreeze,'
)

content = content.replace(
    '    HostCall { call_id: u8 },\n    ObjectFreeze { object_id: ObjectId },',
    '    Trap { service_id: u8 },\n    HostCall { call_id: u8 },\n    ObjectFreeze { object_id: ObjectId },'
)

content = content.replace(
    '            Instruction::HostCall { .. } => Opcode::HostCall,',
    '            Instruction::Trap { .. } => Opcode::Trap,\n            Instruction::HostCall { .. } => Opcode::HostCall,'
)

with open('src/instruction.rs', 'w') as f:
    f.write(content)

print('Done: instruction.rs')
