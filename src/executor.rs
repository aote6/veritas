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

    pub fn read_state(&self, ctx: &mut TransactionContext, state_id: crate::types::StateId) -> Result<Vec<u8>, VeritasError> {
        self.engine.read(ctx, state_id)
    }

    pub fn write_state(&mut self, ctx: &mut TransactionContext, state_id: crate::types::StateId, payload: Vec<u8>) -> Result<(), VeritasError> {
        self.engine.write(ctx, state_id, payload)
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

    /// 执行内核级指令（Kernel Service）。
    /// Machine::step() 将 TRAP 和需要内核服务的指令路由到这里。
    /// 本地指令（算术/跳转/Halt等）由 Machine 自行处理，不经过此方法。
    pub fn execute_instruction(
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
            Instruction::ObjectFreeze { object_id } => {
                self.engine.object_freeze(ctx, *object_id)?;
            }
            Instruction::ObjectLink { from, to, relation } => {
                self.engine.object_link(ctx, *from, *to, *relation)?;
            }
            Instruction::ObjectUnlink { from, to } => {
                self.engine.object_unlink(ctx, *from, *to)?;
            }
            Instruction::CapabilityGrant { holder, permission, resource } => {
                let cap_id = self.engine.capability_grant(ctx, *holder, permission, *resource)?;
                self.engine.attach_capability(ctx, cap_id);
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
            // 本地指令不经过 Executor，Machine 直接处理
            Instruction::Trap { .. }
            | Instruction::HostCall { .. }
            | Instruction::LoadConst { .. }
            | Instruction::Add { .. }
            | Instruction::Sub { .. }
            | Instruction::Cmp { .. }
            | Instruction::LoadStateU64 { .. }
            | Instruction::LoadStateBytes { .. }
            | Instruction::WriteRegister { .. }
            | Instruction::Jmp { .. }
            | Instruction::Jz { .. }
            | Instruction::Jnz { .. }
            | Instruction::Nop
            | Instruction::Halt
            | Instruction::Jn { .. }
            | Instruction::Call { .. }
            | Instruction::Return => {
                // 这些指令由 Machine::step() 本地处理，
                // 不应到达 Executor。如果到达，说明路由错误。
                return Err(VeritasError::EngineError(
                    format!("Local instruction reached Executor: {:?}", inst)
                ));
            }
        }
        Ok(())
    }
}
