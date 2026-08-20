use crate::types::StateId;

/// Machine instruction opcodes. Kernel services use TRAP only (no dedicated opcodes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opcode {
    Read,
    Write,
    Trap,
    HostCall,
    LoadConst,
    Add,
    Sub,
    Cmp,
    LoadStateU64,
    LoadStateBytes,
    WriteRegister,
    Jmp,
    Jz,
    Jnz,
    Nop,
    Halt,
    Jn,
    Call,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Immediate(u64),
    Register(u8),
}

/// Machine instruction set.
///
/// Kernel services are **not** Instruction variants. Use `Trap { service_id }`
/// (service_id 0–13) → KernelCall → Kernel::handle. HostCall is a separate
/// host-boundary primitive (not KernelCall).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Read {
        state_id: Operand,
    },
    Write {
        state_id: Operand,
        payload: Vec<u8>,
    },
    Trap {
        service_id: u8,
    },
    HostCall {
        call_id: u8,
    },
    /// Switch execution context to target object and jump to entry_pc.
    Call {
        object_id: Operand,
        entry_pc: usize,
    },
    /// Return from Call: restore current_object and pc.
    Return,
    Nop,
    LoadConst {
        reg: u8,
        val: u64,
    },
    Add {
        dst: u8,
        src1: u8,
        src2: u8,
    },
    Sub {
        dst: u8,
        src1: u8,
        src2: u8,
    },
    Cmp {
        src1: u8,
        src2: u8,
    },
    LoadStateU64 {
        reg: u8,
        state_id: StateId,
    },
    LoadStateBytes {
        reg: u8,
        state_id: StateId,
    },
    WriteRegister {
        state_id: StateId,
        reg: u8,
    },
    Jmp {
        target: usize,
    },
    Jz {
        target: usize,
    },
    Jnz {
        target: usize,
    },
    Jn {
        target: usize,
    },
    Halt,
}

impl Instruction {
    pub fn opcode(&self) -> Opcode {
        match self {
            Instruction::Read { .. } => Opcode::Read,
            Instruction::Write { .. } => Opcode::Write,
            Instruction::Trap { .. } => Opcode::Trap,
            Instruction::HostCall { .. } => Opcode::HostCall,
            Instruction::Nop => Opcode::Nop,
            Instruction::Halt => Opcode::Halt,
            Instruction::LoadConst { .. } => Opcode::LoadConst,
            Instruction::Add { .. } => Opcode::Add,
            Instruction::Sub { .. } => Opcode::Sub,
            Instruction::Cmp { .. } => Opcode::Cmp,
            Instruction::LoadStateU64 { .. } => Opcode::LoadStateU64,
            Instruction::LoadStateBytes { .. } => Opcode::LoadStateBytes,
            Instruction::WriteRegister { .. } => Opcode::WriteRegister,
            Instruction::Jmp { .. } => Opcode::Jmp,
            Instruction::Jz { .. } => Opcode::Jz,
            Instruction::Jnz { .. } => Opcode::Jnz,
            Instruction::Jn { .. } => Opcode::Jn,
            Instruction::Call { .. } => Opcode::Call,
            Instruction::Return => Opcode::Return,
        }
    }
}
