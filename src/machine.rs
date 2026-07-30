use crate::engine::VeritasEngine;
use crate::executor::Executor;
use crate::program::Program;
use crate::verifier::Verifier;
use crate::types::{TransactionContext, VeritasError, AbortReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineStatus {
    Ready,
    Running,
    Halted,
    Aborted(AbortReason),
}

pub struct Machine<'a> {
    engine: &'a VeritasEngine,
    executor: Executor<'a>,
    program: Program,
    pc: usize,
    status: MachineStatus,
    ctx: TransactionContext,
}

impl<'a> Machine<'a> {
    pub fn new(engine: &'a VeritasEngine, program: Program) -> Result<Self, VeritasError> {
        Verifier::verify(&program)?;
        let ctx = engine.begin();
        let executor = Executor::new(engine);
        Ok(Self {
            engine,
            executor,
            program,
            pc: 0,
            status: MachineStatus::Ready,
            ctx,
        })
    }

    pub fn step(&mut self) -> Result<(), VeritasError> {
        match self.status {
            MachineStatus::Halted | MachineStatus::Aborted(_) => return Ok(()),
            MachineStatus::Ready => self.status = MachineStatus::Running,
            MachineStatus::Running => {}
        }

        if self.pc >= self.program.len() {
            self.status = MachineStatus::Halted;
            return Ok(());
        }

        let instruction = self.program.get(self.pc)
            .ok_or_else(|| VeritasError::EngineError("Invalid PC address".into()))?;

        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, instruction) {
            let reason = match e {
                VeritasError::Abort(r) => r,
                _ => AbortReason::WriteConflict,
            };
            self.engine.abort(&mut self.ctx, reason);
            self.status = MachineStatus::Aborted(reason);
            return Err(e);
        }

        self.pc += 1;

        if self.pc >= self.program.len() {
            self.status = MachineStatus::Halted;
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<(), VeritasError> {
        while self.status == MachineStatus::Ready || self.status == MachineStatus::Running {
            self.step()?;
        }
        Ok(())
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn status(&self) -> &MachineStatus {
        &self.status
    }

    pub fn is_halted(&self) -> bool {
        matches!(self.status, MachineStatus::Halted | MachineStatus::Aborted(_))
    }
}
