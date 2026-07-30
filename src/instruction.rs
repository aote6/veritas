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
    Commit,
    Abort,
}

#[derive(Debug, Clone, PartialEq)]
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
            Instruction::Commit => Opcode::Commit,
            Instruction::Abort { .. } => Opcode::Abort,
        }
    }
}
