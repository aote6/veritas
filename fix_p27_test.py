with open('src/engine.rs', 'r') as f:
    content = f.read()

# Fix import
old_import = '    use crate::machine::Machine;'
new_import = '''    use crate::machine::{Machine, MachineStatus};'''
content = content.replace(old_import, new_import)

# Fix decode - returns (Instruction, usize)
old_decode = '''            let bytes = inst.encode().unwrap();
            let decoded = Instruction::decode(&bytes).unwrap();
            match decoded {'''

new_decode = '''            let bytes = inst.encode().unwrap();
            let (decoded, _len) = Instruction::decode(&bytes).unwrap();
            match decoded {'''

content = content.replace(old_decode, new_decode)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
