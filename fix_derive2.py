with open('src/machine.rs', 'r') as f:
    content = f.read()

content = content.replace(
    '''#[derive(Debug, Clone)]
pub struct RegisterFile {
#[derive(Debug, Clone)]
    regs: [RegisterValue; 8],
}''',
    '''#[derive(Debug, Clone)]
pub struct RegisterFile {
    regs: [RegisterValue; 8],
}'''
)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
