with open('src/instruction_codec.rs', 'r') as f:
    content = f.read()

# Add opcode constant
content = content.replace(
    '    pub const OBJECT_UNLINK: u8 = 0x39;',
    '    pub const OBJECT_UNLINK: u8 = 0x39;\n    pub const OBJECT_FREEZE: u8 = 0x3A;'
)

# Add encode
content = content.replace(
    '            Instruction::ObjectUnlink { from, to } => {\n                buf.push(opcodes::OBJECT_UNLINK);',
    '            Instruction::ObjectFreeze { object_id } => {\n                buf.push(opcodes::OBJECT_FREEZE);\n                buf.extend_from_slice(&object_id.to_le_bytes());\n            }\n            Instruction::ObjectUnlink { from, to } => {\n                buf.push(opcodes::OBJECT_UNLINK);'
)

# Add decode
content = content.replace(
    '            opcodes::OBJECT_UNLINK => {',
    '            opcodes::OBJECT_FREEZE => {\n                check!(8);\n                let object_id = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap());\n                pos += 8;\n                Instruction::ObjectFreeze { object_id }\n            }\n            opcodes::OBJECT_UNLINK => {'
)

with open('src/instruction_codec.rs', 'w') as f:
    f.write(content)

print('Done: instruction_codec.rs')
