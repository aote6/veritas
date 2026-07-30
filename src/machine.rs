use crate::engine::VeritasEngine;
use crate::executor::Executor;
use crate::program::Program;
use crate::verifier::Verifier;
use crate::types::{TransactionContext, VeritasError, AbortReason};
use crate::instruction::Instruction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineStatus {
    Ready,
    Running,
    Halted,
    Aborted(AbortReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterValue {
    Empty,
    U64(u64),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlagsRegister {
    pub zero: bool,
    pub negative: bool,
    pub overflow: bool,
    pub carry: bool,
}

pub struct RegisterFile {
    regs: [RegisterValue; 8],
}

impl RegisterFile {
    pub fn new() -> Self {
        Self { regs: std::array::from_fn(|_| RegisterValue::Empty) }
    }

    pub fn get(&self, reg: u8) -> &RegisterValue {
        &self.regs[(reg as usize) % 8]
    }

    pub fn set(&mut self, reg: u8, val: RegisterValue) {
        self.regs[(reg as usize) % 8] = val;
    }

    pub fn get_u64(&self, reg: u8) -> u64 {
        match self.get(reg) {
            RegisterValue::U64(v) => *v,
            _ => 0,
        }
    }
}

pub struct Machine<'a> {
    engine: &'a VeritasEngine,
    executor: Executor<'a>,
    program: Program,
    pc: usize,
    status: MachineStatus,
    registers: RegisterFile,
    flags: FlagsRegister,
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
            registers: RegisterFile::new(),
            flags: FlagsRegister::default(),
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

        // P13.1: 本地指令直接在 Machine 内部消化
        match instruction {
            crate::instruction::Instruction::LoadConst { reg, val } => {
                self.registers.set(*reg, RegisterValue::U64(*val));
                self.pc += 1;
                if self.pc >= self.program.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Add { dst, src1, src2 } => {
                let v1 = self.registers.get_u64(*src1);
                let v2 = self.registers.get_u64(*src2);
                let (res, overflow) = v1.overflowing_add(v2);
                self.registers.set(*dst, RegisterValue::U64(res));
                self.flags.zero = res == 0;
                self.flags.overflow = overflow;
                self.flags.negative = false;
                self.pc += 1;
                if self.pc >= self.program.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Sub { dst, src1, src2 } => {
                let v1 = self.registers.get_u64(*src1);
                let v2 = self.registers.get_u64(*src2);
                let (res, overflow) = v1.overflowing_sub(v2);
                self.registers.set(*dst, RegisterValue::U64(res));
                self.flags.zero = res == 0;
                self.flags.overflow = overflow;
                self.flags.negative = v1 < v2;
                self.pc += 1;
                if self.pc >= self.program.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Cmp { src1, src2 } => {
                let v1 = self.registers.get_u64(*src1);
                let v2 = self.registers.get_u64(*src2);
                self.flags.zero = v1 == v2;
                self.flags.negative = v1 < v2;
                self.flags.overflow = false;
                self.pc += 1;
                if self.pc >= self.program.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            _ => {}
        }

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

    pub fn registers(&self) -> &RegisterFile { &self.registers }

    pub fn flags(&self) -> &FlagsRegister { &self.flags }

    pub fn status(&self) -> &MachineStatus {
        &self.status
    }

    pub fn is_halted(&self) -> bool {
        matches!(self.status, MachineStatus::Halted | MachineStatus::Aborted(_))
    }
}
