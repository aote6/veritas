# Step 2: instruction_codec.rs - add TRAP opcode and encode/decode
with open('src/instruction_codec.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '    pub const HOST_CALL: u8 = 0x40;',
    '    pub const HOST_CALL: u8 = 0x40;\n    pub const TRAP: u8 = 0x41;'
)

content = content.replace(
    '            Instruction::HostCall { call_id } => {\n                buf.push(opcodes::HOST_CALL);\n                buf.push(*call_id);\n            }',
    '            Instruction::Trap { service_id } => {\n                buf.push(opcodes::TRAP);\n                buf.push(*service_id);\n            }\n            Instruction::HostCall { call_id } => {\n                buf.push(opcodes::HOST_CALL);\n                buf.push(*call_id);\n            }'
)

content = content.replace(
    '            opcodes::HOST_CALL => {',
    '            opcodes::TRAP => {\n                check!(1);\n                let service_id = bytes[pos];\n                pos += 1;\n                Instruction::Trap { service_id }\n            }\n            opcodes::HOST_CALL => {'
)

with open('src/instruction_codec.rs', 'w') as f:
    f.write(content)

print('Done: instruction_codec.rs')
