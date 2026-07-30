use crate::types::{StateId, ObjectId, RelationKind};
use crate::types::AbortReason;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opcode {
    Read,
    Write,
    Effect,
    ObjectBirth,
    ObjectDeath,
    ObjectLink,
    CapabilityGrant,
    Savepoint,
    RollbackTo,
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
    Commit,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Read { state_id: StateId },
    Write { state_id: StateId, payload: Vec<u8> },
    Effect { payload: Vec<u8> },
    ObjectBirth { object_id: ObjectId },
    ObjectDeath { object_id: ObjectId },
    ObjectLink { from: ObjectId, to: ObjectId, relation: RelationKind },
    CapabilityGrant { holder: ObjectId, permission: String, resource: StateId },
    Savepoint { name: String },
    RollbackTo { name: String },
    Nop,
    LoadConst { reg: u8, val: u64 },
    Add { dst: u8, src1: u8, src2: u8 },
    Sub { dst: u8, src1: u8, src2: u8 },
    Cmp { src1: u8, src2: u8 },
    LoadStateU64 { reg: u8, state_id: StateId },
    LoadStateBytes { reg: u8, state_id: StateId },
    WriteRegister { state_id: StateId, reg: u8 },
    Jmp { target: usize },
    Jz { target: usize },
    Jnz { target: usize },
    Jn { target: usize },
    Halt,
    Commit,
    Abort { reason: AbortReason },
}

impl Instruction {
    pub fn opcode(&self) -> Opcode {
        match self {
            Instruction::Read { .. } => Opcode::Read,
            Instruction::Write { .. } => Opcode::Write,
            Instruction::Effect { .. } => Opcode::Effect,
            Instruction::ObjectBirth { .. } => Opcode::ObjectBirth,
            Instruction::ObjectDeath { .. } => Opcode::ObjectDeath,
            Instruction::ObjectLink { .. } => Opcode::ObjectLink,
            Instruction::CapabilityGrant { .. } => Opcode::CapabilityGrant,
            Instruction::Savepoint { .. } => Opcode::Savepoint,
            Instruction::RollbackTo { .. } => Opcode::RollbackTo,
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
            Instruction::Commit => Opcode::Commit,
            Instruction::Abort { .. } => Opcode::Abort,
        }
    }
}
