with open('src/instruction_codec.rs', 'r') as f:
    content = f.read()

# Add ObjectUnlink encode
old_encode = '''            Instruction::ObjectLink { from, to, relation } => {
                buf.push(opcodes::OBJECT_LINK);
                buf.extend_from_slice(&from.to_le_bytes());
                buf.extend_from_slice(&to.to_le_bytes());
                buf.push(*relation as u8);
            }'''

new_encode = '''            Instruction::ObjectLink { from, to, relation } => {
                buf.push(opcodes::OBJECT_LINK);
                buf.extend_from_slice(&from.to_le_bytes());
                buf.extend_from_slice(&to.to_le_bytes());
                buf.push(*relation as u8);
            }
            Instruction::ObjectUnlink { from, to } => {
                buf.push(opcodes::OBJECT_UNLINK);
                buf.extend_from_slice(&from.to_le_bytes());
                buf.extend_from_slice(&to.to_le_bytes());
            }'''

content = content.replace(old_encode, new_encode)

# Add ObjectUnlink decode
old_decode = '''                Instruction::ObjectLink { from, to, relation: unsafe { std::mem::transmute(rel) } }
            }'''

new_decode = '''                Instruction::ObjectLink { from, to, relation: unsafe { std::mem::transmute(rel) } }
            }
            opcodes::OBJECT_UNLINK => {
                check!(16);
                let from = u64::from_le_bytes(bytes[pos..pos+8].try_into().unwrap());
                let to = u64::from_le_bytes(bytes[pos+8..pos+16].try_into().unwrap());
                pos += 16;
                Instruction::ObjectUnlink { from, to }
            }'''

content = content.replace(old_decode, new_decode)

with open('src/instruction_codec.rs', 'w') as f:
    f.write(content)

print('Done')
