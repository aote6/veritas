# Step 2: Add HostCall variant to Instruction enum
with open('src/instruction.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    ObjectFreeze,\n    ObjectLink,',
    '    HostCall,\n    ObjectFreeze,\n    ObjectLink,'
)

content = content.replace(
    '    ObjectFreeze { object_id: ObjectId },\n    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },',
    '    HostCall { call_id: u8 },\n    ObjectFreeze { object_id: ObjectId },\n    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },'
)

content = content.replace(
    '            Instruction::ObjectFreeze { .. } => Opcode::ObjectFreeze,',
    '            Instruction::HostCall { .. } => Opcode::HostCall,\n            Instruction::ObjectFreeze { .. } => Opcode::ObjectFreeze,'
)

with open('src/instruction.rs', 'w') as f:
    f.write(content)

print('Done: instruction.rs')
