# Step 1: Add HostCall opcode constant
with open('src/instruction_codec.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    pub const OBJECT_FREEZE: u8 = 0x3A;',
    '    pub const OBJECT_FREEZE: u8 = 0x3A;\n    pub const HOST_CALL: u8 = 0x40;'
)

# Add HostCall encode
content = content.replace(
    '            Instruction::ObjectFreeze { object_id } => {\n                buf.push(opcodes::OBJECT_FREEZE);\n                buf.extend_from_slice(&object_id.to_le_bytes());\n            }',
    '            Instruction::HostCall { call_id } => {\n                buf.push(opcodes::HOST_CALL);\n                buf.push(*call_id);\n            }\n            Instruction::ObjectFreeze { object_id } => {\n                buf.push(opcodes::OBJECT_FREEZE);\n                buf.extend_from_slice(&object_id.to_le_bytes());\n            }'
)

# Add HostCall decode
content = content.replace(
    '            opcodes::OBJECT_FREEZE => {',
    '            opcodes::HOST_CALL => {\n                check!(1);\n                let call_id = bytes[pos];\n                pos += 1;\n                Instruction::HostCall { call_id }\n            }\n            opcodes::OBJECT_FREEZE => {'
)

with open('src/instruction_codec.rs', 'w') as f:
    f.write(content)

print('Done: instruction_codec.rs')
