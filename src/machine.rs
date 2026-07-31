use crate::engine::VeritasEngine;
use crate::executor::Executor;
use crate::program::Program;
use crate::memory::Memory;
use crate::verifier::Verifier;
use crate::types::{TransactionContext, VeritasError, AbortReason};
use crate::instruction::Instruction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineStatus {
    Ready,
    Running,
    Halted,
    Aborted(AbortReason),
    Trapped(crate::types::TrapReason),
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

impl FlagsRegister {
    pub fn new() -> Self {
        Self { zero: false, negative: false, overflow: false, carry: false }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

pub struct RegisterFile {
    regs: [RegisterValue; 8],
}

impl RegisterFile {
    pub fn new() -> Self {
        Self { regs: std::array::from_fn(|_| RegisterValue::Empty) }
    }

    pub fn reset(&mut self) {
        self.regs = std::array::from_fn(|_| RegisterValue::Empty);
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


#[derive(Debug, Clone, Copy)]
pub struct ExecutionConfig {
    pub max_cycles: u64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self { max_cycles: 1_000_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionResult {
    Halted { cycles: u64 },
    CycleLimitReached { cycles: u64 },
    Aborted(String),
}

pub struct Machine<'a> {
    engine: &'a VeritasEngine,
    executor: Executor<'a>,
    program: Program,
    ram: Memory,
    pc: usize,
    status: MachineStatus,
    registers: RegisterFile,
    flags: FlagsRegister,
    ctx: TransactionContext,
    trap_frame: Option<crate::types::TrapFrame>,
    pub execution: crate::execution::ExecutionContext,
}

impl<'a> Machine<'a> {
    fn record_trace(&mut self, pc_before: usize, regs_before: [u64; 8], instruction: &crate::instruction::Instruction, consumed: usize) {
        let regs_after = [
            self.registers.get_u64(0), self.registers.get_u64(1),
            self.registers.get_u64(2), self.registers.get_u64(3),
            self.registers.get_u64(4), self.registers.get_u64(5),
            self.registers.get_u64(6), self.registers.get_u64(7),
        ];
        self.execution.record_instruction(crate::trace::InstructionTrace {
            pc: pc_before,
            opcode: instruction.opcode() as u8,
            instruction: instruction.clone(),
            registers_before: regs_before,
            registers_after: regs_after,
            state_reads: vec![],
            state_writes: vec![],
        });
    }
    pub fn set_pc(&mut self, pc: usize) { self.pc = pc; }
    pub fn ram_mut(&mut self) -> &mut Memory { &mut self.ram }

    pub fn new(engine: &'a VeritasEngine) -> Self {
        let executor = Executor::new(engine);
        let ctx = engine.begin();
        Self {
            engine,
            executor,
            program: Program::new(),
            ram: Memory::new(65536),
            pc: 0,
            status: MachineStatus::Ready,
            registers: RegisterFile::new(),
            flags: FlagsRegister::default(),
            ctx,
            trap_frame: None,
            execution: crate::execution::ExecutionContext::new(0, 0),
        }
    }

    pub fn step(&mut self) -> Result<(), VeritasError> {
        match self.status {
            MachineStatus::Halted | MachineStatus::Aborted(_) | MachineStatus::Trapped(_) => return Ok(()),
            MachineStatus::Ready => self.status = MachineStatus::Running,
            MachineStatus::Running => {}
        }

        if self.pc >= self.ram.len() {
            let reason = crate::types::TrapReason::MemoryFault { addr: self.pc, size: 1 };
            self.trap_frame = Some(crate::types::TrapFrame {
                pc: self.pc, reason: reason.clone(), cycles: 0,
            });
            self.status = MachineStatus::Trapped(reason);
            return Ok(());
        }

        let stream = match self.ram.slice_from(self.pc) {
            Ok(s) => s,
            Err(_) => {
                let reason = crate::types::TrapReason::MemoryFault { addr: self.pc, size: 1 };
                self.trap_frame = Some(crate::types::TrapFrame {
                    pc: self.pc, reason: reason.clone(), cycles: 0,
                });
                self.status = MachineStatus::Trapped(reason);
                return Ok(());
            }
        };

        let pc_before = self.pc;
        let regs_before = [
            self.registers.get_u64(0), self.registers.get_u64(1),
            self.registers.get_u64(2), self.registers.get_u64(3),
            self.registers.get_u64(4), self.registers.get_u64(5),
            self.registers.get_u64(6), self.registers.get_u64(7),
        ];

        let (instruction, consumed) = match crate::instruction::Instruction::decode(stream) {
            Ok(v) => v,
            Err(_) => {
                let reason = crate::types::TrapReason::InvalidEncoding { pc: self.pc };
                self.trap_frame = Some(crate::types::TrapFrame {
                    pc: self.pc, reason: reason.clone(), cycles: 0,
                });
                self.status = MachineStatus::Trapped(reason);
                return Ok(());
            }
        };

        // P13.1: 本地指令直接在 Machine 内部消化
        match instruction {
            crate::instruction::Instruction::LoadConst { reg, val } => {
                self.registers.set(reg, RegisterValue::U64(val));
                self.pc += consumed;
                self.record_trace(pc_before, regs_before, &instruction, consumed);
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Add { dst, src1, src2 } => {
                let v1 = self.registers.get_u64(src1);
                let v2 = self.registers.get_u64(src2);
                let (res, overflow) = v1.overflowing_add(v2);
                self.registers.set(dst, RegisterValue::U64(res));
                self.flags.zero = res == 0;
                self.flags.overflow = overflow;
                self.flags.negative = false;
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Sub { dst, src1, src2 } => {
                let v1 = self.registers.get_u64(src1);
                let v2 = self.registers.get_u64(src2);
                let (res, overflow) = v1.overflowing_sub(v2);
                self.registers.set(dst, RegisterValue::U64(res));
                self.flags.zero = res == 0;
                self.flags.overflow = overflow;
                self.flags.negative = v1 < v2;
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Cmp { src1, src2 } => {
                let v1 = self.registers.get_u64(src1);
                let v2 = self.registers.get_u64(src2);
                self.flags.zero = v1 == v2;
                self.flags.negative = v1 < v2;
                self.flags.overflow = false;
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::LoadStateU64 { reg, state_id } => {
                let bytes = self.executor.read_state(&mut self.ctx, state_id)?;
                let mut arr = [0u8; 8];
                let len = bytes.len().min(8);
                arr[..len].copy_from_slice(&bytes[..len]);
                let val = u64::from_le_bytes(arr);
                self.registers.set(reg, RegisterValue::U64(val));
            }
            Instruction::LoadStateBytes { reg, state_id } => {
                let bytes = self.executor.read_state(&mut self.ctx, state_id)?;
                self.registers.set(reg, RegisterValue::Bytes(bytes));
            }
            Instruction::WriteRegister { state_id, reg } => {
                let payload = match self.registers.get(reg) {
                    RegisterValue::U64(v) => v.to_le_bytes().to_vec(),
                    RegisterValue::Bytes(b) => b.clone(),
                    RegisterValue::Empty => vec![],
                };
                self.executor.write_state(&mut self.ctx, state_id, payload.clone())?;
                self.execution.record_write(state_id, payload.clone());
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Nop => {
                self.pc += consumed;
                self.record_trace(pc_before, regs_before, &instruction, consumed);
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::Halt => {
                self.pc += consumed;
                self.record_trace(pc_before, regs_before, &instruction, consumed);
                self.status = MachineStatus::Halted;
                return Ok(());
            }
            Instruction::Jmp { target } => {
                self.pc = target;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Jz { target } => {
                if self.flags.zero { self.pc = target; } else { self.pc += consumed; }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::Jnz { target } => {
                if !self.flags.zero { self.pc = target; } else { self.pc += consumed; }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::Jn { target } => {
                if self.flags.negative { self.pc = target; } else { self.pc += consumed; }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            _ => {}
        }

        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {
            let reason = match e {
                VeritasError::Abort(r) => r,
                _ => AbortReason::WriteConflict,
            };
            self.engine.abort(&mut self.ctx, reason);
            self.status = MachineStatus::Aborted(reason);
            return Err(e);
        }

        self.pc += consumed;

        // P19.1: 记录指令执行 trace
        let regs_after = [
            self.registers.get_u64(0), self.registers.get_u64(1),
            self.registers.get_u64(2), self.registers.get_u64(3),
            self.registers.get_u64(4), self.registers.get_u64(5),
            self.registers.get_u64(6), self.registers.get_u64(7),
        ];
        self.execution.record_instruction(crate::trace::InstructionTrace {
            pc: self.pc.saturating_sub(consumed),
            opcode: instruction.opcode() as u8,
            instruction: instruction.clone(),
            registers_before: regs_before,
            registers_after: regs_after,
            state_reads: vec![],
            state_writes: vec![],
        });

        if self.pc >= self.ram.len() {
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

    pub fn run_with_config(&mut self, config: ExecutionConfig) -> Result<ExecutionResult, VeritasError> {
        let mut cycles = 0;
        while self.status == MachineStatus::Running || self.status == MachineStatus::Ready {
            if cycles >= config.max_cycles {
                return Ok(ExecutionResult::CycleLimitReached { cycles });
            }
            if self.status == MachineStatus::Ready {
                self.status = MachineStatus::Running;
            }
            self.step()?;
            cycles += 1;
            if self.is_halted() {
                return Ok(ExecutionResult::Halted { cycles });
            }
        }
        Ok(ExecutionResult::Halted { cycles })
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn with_program(mut self, program: Program) -> Result<Self, VeritasError> {
        Verifier::verify(&program)?;
        self.program = program;
        self.status = MachineStatus::Ready;
        self.trap_frame = None;
        self.execution = crate::execution::ExecutionContext::new(0, 0);
        Ok(self)
    }

    pub fn boot(&mut self, image: crate::program::ProgramImage) -> Result<(), VeritasError> {
        self.registers.reset();
        self.flags.reset();
        self.ram.clear();
        let prog_hash = image.instructions.iter().fold(0u64, |h, inst| {
            let bytes = inst.encode().unwrap_or_default();
            bytes.iter().fold(h, |acc, &b| acc.wrapping_mul(0x100000001b3) ^ (b as u64))
        });

        let mut addr = 0usize;
        for inst in &image.instructions {
            let encoded = inst.encode()?;
            self.ram.write_bytes(addr, &encoded)
                .map_err(|e| VeritasError::EngineError(e))?;
            addr += encoded.len();
        }

        self.pc = image.entry_point as usize;
        self.status = MachineStatus::Running;
        self.trap_frame = None;
        self.execution = crate::execution::ExecutionContext::new(
            prog_hash,
            self.engine.state_root(),
        );
        Ok(())
    }


    pub fn boot_bytes(&mut self, bytes: &[u8]) -> Result<(), VeritasError> {
        let image = crate::program::ProgramImage::decode(bytes)?;
        self.boot(image)
    }

    pub fn registers(&self) -> &RegisterFile { &self.registers }

    pub fn flags(&self) -> &FlagsRegister { &self.flags }

    pub fn status(&self) -> &MachineStatus {
        &self.status
    }

    pub fn trap_frame(&self) -> Option<&crate::types::TrapFrame> { self.trap_frame.as_ref() }

    pub fn trace_hash(&self) -> u64 { self.execution.trace.trace_hash() }

    pub fn execution_receipt(&self) -> crate::receipt::ExecutionReceipt {
        self.execution.finalize(self.state_root())
    }

    pub fn state_root(&self) -> u64 { self.engine.state_root() }

    pub fn is_halted(&self) -> bool {
        matches!(self.status, MachineStatus::Halted | MachineStatus::Aborted(_))
    }
}
