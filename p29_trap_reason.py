with open('src/types.rs', 'r') as f:
    content = f.read()

old = '''pub enum TrapReason {
    InvalidOpcode { opcode: u8 },
    InvalidEncoding { pc: usize },
    MemoryFault { addr: usize, size: usize },
    DivisionByZero,
    ArithmeticOverflow,
    IllegalInstruction { opcode: u8 },
}'''

new = '''pub enum TrapReason {
    InvalidOpcode { opcode: u8 },
    InvalidEncoding { pc: usize },
    MemoryFault { addr: usize, size: usize },
    DivisionByZero,
    ArithmeticOverflow,
    IllegalInstruction { opcode: u8 },
    /// P29: Capability检查失败，硬件级越权拦截
    AccessDenied { pc: usize },
}'''

content = content.replace(old, new)

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Done: TrapReason added')
