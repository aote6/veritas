use crate::engine::VeritasEngine;
use crate::executor::Executor;
use crate::program::Program;
use crate::verifier::Verifier;
use crate::types::{TransactionContext, VeritasError, AbortReason};

pub struct Machine<'a> {
    engine: &'a VeritasEngine,
    program: Program,
    pc: usize,
    halted: bool,
    ctx: TransactionContext,
}

impl<'a> Machine<'a> {
    pub fn new(engine: &'a VeritasEngine, program: Program) -> Result<Self, VeritasError> {
        Verifier::verify(&program)?;
        let ctx = engine.begin();
        Ok(Self { engine, program, pc: 0, halted: false, ctx })
    }

    pub fn step(&mut self) -> Result<(), VeritasError> {
        if self.halted {
            return Ok(());
        }
        if self.pc >= self.program.len() {
            self.halted = true;
            return Ok(());
        }

        let inst = self.program.get(self.pc)
            .ok_or_else(|| VeritasError::EngineError("Invalid PC".into()))?;

        let executor = Executor::new(self.engine);
        if let Err(e) = executor.execute_instruction(&mut self.ctx, inst) {
            self.engine.abort(&mut self.ctx, AbortReason::WriteConflict);
            self.halted = true;
            return Err(e);
        }

        self.pc += 1;
        if self.pc >= self.program.len() {
            self.halted = true;
        }
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), VeritasError> {
        while !self.halted {
            self.step()?;
        }
        Ok(())
    }

    pub fn pc(&self) -> usize { self.pc }
    pub fn halted(&self) -> bool { self.halted }
}
