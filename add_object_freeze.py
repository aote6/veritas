# Step 1: instruction.rs - add ObjectFreeze variant
with open('src/instruction.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    ObjectDeath,\n    ObjectLink,',
    '    ObjectDeath,\n    ObjectFreeze,\n    ObjectLink,'
)

content = content.replace(
    '    ObjectDeath { object_id: ObjectId },\n    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },',
    '    ObjectDeath { object_id: ObjectId },\n    ObjectFreeze { object_id: ObjectId },\n    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },'
)

content = content.replace(
    '            Instruction::ObjectDeath { .. } => Opcode::ObjectDeath,',
    '            Instruction::ObjectDeath { .. } => Opcode::ObjectDeath,\n            Instruction::ObjectFreeze { .. } => Opcode::ObjectFreeze,'
)

with open('src/instruction.rs', 'w') as f:
    f.write(content)

print('Done: instruction.rs')
