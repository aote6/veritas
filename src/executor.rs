use crate::engine::VeritasEngine;
use crate::program::Program;
use crate::instruction::Instruction;
use crate::verifier::Verifier;
use crate::types::{TransactionContext, VeritasError, AbortReason};

pub struct Executor<'a> {
    engine: &'a VeritasEngine,
}

impl<'a> Executor<'a> {
    pub fn new(engine: &'a VeritasEngine) -> Self {
        Self { engine }
    }

    pub fn run_program(&self, program: &Program) -> Result<(), VeritasError> {
        Verifier::verify(program)?;
        let mut ctx = self.engine.begin();

        for inst in &program.instructions {
            if let Err(e) = self.execute_instruction(&mut ctx, inst) {
                self.engine.abort(&mut ctx, AbortReason::WriteConflict);
                return Err(e);
            }
        }

        Ok(())
    }

    fn execute_instruction(
        &self,
        ctx: &mut TransactionContext,
        inst: &Instruction,
    ) -> Result<(), VeritasError> {
        match inst {
            Instruction::Read { state_id } => {
                self.engine.read(ctx, *state_id)?;
            }
            Instruction::Write { state_id, payload } => {
                self.engine.write(ctx, *state_id, payload.clone())?;
            }
            Instruction::Effect { payload } => {
                self.engine.effect(ctx, payload.clone())?;
            }
            Instruction::ObjectBirth { object_id } => {
                self.engine.object_birth(ctx, *object_id)?;
            }
            Instruction::ObjectDeath { object_id } => {
                self.engine.object_death(ctx, *object_id)?;
            }
            Instruction::ObjectLink { from, to, relation } => {
                self.engine.object_link(ctx, *from, *to, *relation)?;
            }
            Instruction::CapabilityGrant { holder, permission, resource } => {
                self.engine.capability_grant(ctx, *holder, permission, *resource)?;
            }
            Instruction::Savepoint { name } => {
                self.engine.savepoint(ctx, name)?;
            }
            Instruction::RollbackTo { name } => {
                self.engine.rollback_to(ctx, name)?;
            }
            Instruction::Commit => {
                self.engine.commit(ctx)?;
            }
            Instruction::Abort { reason } => {
                self.engine.abort(ctx, *reason);
                return Err(VeritasError::Abort(*reason));
            }
        }
        Ok(())
    }
}
