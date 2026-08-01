with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''mod p24_object_isolation_tests {
    use super::*;
    use crate::program::Program;
    use crate::instruction::Instruction;
    use crate::program::ProgramImage;'''

new = '''mod p24_object_isolation_tests {
    use super::*;
    use crate::program::{Program, ProgramImage};
    use crate::instruction::Instruction;
    use crate::machine::Machine;
    use crate::types::{VeritasError, AbortReason};'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
