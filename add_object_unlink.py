with open('src/instruction.rs', 'r') as f:
    content = f.read()

# Add ObjectUnlink variant
content = content.replace(
    '    ObjectLink,',
    '    ObjectLink,\n    ObjectUnlink,'
)

# Add ObjectUnlink instruction struct
content = content.replace(
    '    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },',
    '    ObjectLink { from: ObjectId, to: ObjectId, relation: LinkType },\n    ObjectUnlink { from: ObjectId, to: ObjectId },'
)

# Add ObjectUnlink opcode mapping
content = content.replace(
    '            Instruction::ObjectLink { .. } => Opcode::ObjectLink,',
    '            Instruction::ObjectLink { .. } => Opcode::ObjectLink,\n            Instruction::ObjectUnlink { .. } => Opcode::ObjectUnlink,'
)

with open('src/instruction.rs', 'w') as f:
    f.write(content)

print('Done: instruction.rs')
