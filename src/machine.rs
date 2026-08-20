use crate::instruction::Instruction;
use crate::memory::Memory;
use crate::program::Program;
use crate::types::{AbortReason, TransactionContext, VeritasError};
use crate::verifier::Verifier;
use crate::ObjectId;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineStatus {
    Ready,
    Running,
    Halted,
    Aborted(AbortReason),
    Trapped(crate::types::TrapReason),
}

/// Map kernel TrapResult::Error code to TrapReason.
/// `pc` is taken from the call site so trap frames carry accurate location.
fn map_trap_code(code: u8, pc: usize) -> crate::types::TrapReason {
    match code {
        crate::kernel::TRAP_ERR_ACCESS_DENIED => crate::types::TrapReason::AccessDenied { pc },
        crate::kernel::TRAP_ERR_ENGINE => crate::types::TrapReason::EngineError { pc },
        crate::kernel::TRAP_ERR_MEMORY_FAULT => {
            // Reserved: no real memory fault source exists yet.
            // Do not fabricate addr/size. Treat as unknown until a real source exists.
            crate::types::TrapReason::UnknownKernelError { code, pc }
        }
        crate::kernel::TRAP_ERR_WRITE_CONFLICT => crate::types::TrapReason::WriteConflict { pc },
        crate::kernel::TRAP_ERR_PERMISSION_DENIED => crate::types::TrapReason::AccessDenied { pc },
        crate::kernel::TRAP_ERR_STATE_NOT_FOUND => crate::types::TrapReason::StateNotFound { pc },
        _ => crate::types::TrapReason::UnknownKernelError { code, pc },
    }
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
        Self {
            zero: false,
            negative: false,
            overflow: false,
            carry: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Debug, Clone)]
pub struct RegisterFile {
    regs: [RegisterValue; 8],
}

impl RegisterFile {
    pub fn new() -> Self {
        Self {
            regs: std::array::from_fn(|_| RegisterValue::Empty),
        }
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
        Self {
            max_cycles: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionResult {
    Halted { cycles: u64 },
    CycleLimitReached { cycles: u64 },
    Aborted(String),
}

#[derive(Debug)]
struct CallFrame {
    return_pc: usize,
    parent_object: ObjectId,
    registers: RegisterFile,
    caller_capability_context: ObjectId,
}

pub struct Machine {
    kernel: Arc<crate::kernel::Kernel>,

    program: Program,
    ram: Memory,
    pc: usize,
    status: MachineStatus,
    registers: RegisterFile,
    flags: FlagsRegister,
    ctx: TransactionContext,
    trap_frame: Option<crate::types::TrapFrame>,
    pub execution: crate::execution::ExecutionContext,
    call_stack: Vec<CallFrame>,
}

impl Machine {
    fn record_trace(
        &mut self,
        pc_before: usize,
        regs_before: [u64; 8],
        instruction: &crate::instruction::Instruction,
        _consumed: usize,
    ) {
        let regs_after = [
            self.registers.get_u64(0),
            self.registers.get_u64(1),
            self.registers.get_u64(2),
            self.registers.get_u64(3),
            self.registers.get_u64(4),
            self.registers.get_u64(5),
            self.registers.get_u64(6),
            self.registers.get_u64(7),
        ];
        self.execution
            .record_instruction(crate::trace::InstructionTrace {
                pc: pc_before,
                opcode: instruction.opcode() as u8,
                instruction: instruction.clone(),
                registers_before: regs_before,
                registers_after: regs_after,
                state_reads: vec![],
                state_writes: vec![],
            });
    }
    pub fn set_pc(&mut self, pc: usize) {
        self.pc = pc;
    }

    fn resolve_operand(&self, op: &crate::instruction::Operand) -> u64 {
        match op {
            crate::instruction::Operand::Immediate(v) => *v,
            crate::instruction::Operand::Register(r) => self.registers.get_u64(*r),
        }
    }
    pub fn current_object(&self) -> ObjectId {
        self.ctx.current_object
    }
    /// Set the execution identity for subsequent instructions (CALL/Read/Write).
    /// Used by tests and by future TRAP-based context switch paths.
    pub fn set_execution_object(&mut self, object_id: ObjectId) {
        self.ctx.enter_object(object_id);
        self.ctx.capability_context = object_id;
    }
    pub fn ram_mut(&mut self) -> &mut Memory {
        &mut self.ram
    }

    pub fn new(kernel: Arc<crate::kernel::Kernel>) -> Self {
        let ctx = kernel.begin();
        Self {
            kernel,
            program: Program::new(),
            ram: Memory::new(65536),
            pc: 0,
            status: MachineStatus::Ready,
            registers: RegisterFile::new(),
            flags: FlagsRegister::default(),
            ctx,
            trap_frame: None,
            execution: crate::execution::ExecutionContext::new(0, [0u8; 32]),
            call_stack: Vec::new(),
        }
    }

    pub fn step(&mut self) -> Result<(), VeritasError> {
        match self.status {
            MachineStatus::Halted | MachineStatus::Aborted(_) | MachineStatus::Trapped(_) => {
                return Ok(())
            }
            MachineStatus::Ready => self.status = MachineStatus::Running,
            MachineStatus::Running => {}
        }

        if self.pc >= self.ram.len() {
            let reason = crate::types::TrapReason::MemoryFault {
                addr: self.pc,
                size: 1,
            };
            self.trap_frame = Some(crate::types::TrapFrame {
                pc: self.pc,
                reason: reason.clone(),
                cycles: 0,
            });
            self.status = MachineStatus::Trapped(reason);
            return Ok(());
        }

        let stream = match self.ram.slice_from(self.pc) {
            Ok(s) => s,
            Err(_) => {
                let reason = crate::types::TrapReason::MemoryFault {
                    addr: self.pc,
                    size: 1,
                };
                self.trap_frame = Some(crate::types::TrapFrame {
                    pc: self.pc,
                    reason: reason.clone(),
                    cycles: 0,
                });
                self.status = MachineStatus::Trapped(reason);
                return Ok(());
            }
        };

        let pc_before = self.pc;
        let regs_before = [
            self.registers.get_u64(0),
            self.registers.get_u64(1),
            self.registers.get_u64(2),
            self.registers.get_u64(3),
            self.registers.get_u64(4),
            self.registers.get_u64(5),
            self.registers.get_u64(6),
            self.registers.get_u64(7),
        ];

        let (instruction, consumed) = match crate::instruction::Instruction::decode(stream) {
            Ok(v) => v,
            Err(_) => {
                let reason = crate::types::TrapReason::InvalidEncoding { pc: self.pc };
                self.trap_frame = Some(crate::types::TrapFrame {
                    pc: self.pc,
                    reason: reason.clone(),
                    cycles: 0,
                });
                self.status = MachineStatus::Trapped(reason);
                return Ok(());
            }
        };

        self.execution
            .begin_instruction(pc_before, regs_before, instruction.clone());

        // P13.1: 本地指令直接在 Machine 内部消化
        // Clone instruction for record_trace before destructuring moves fields
        let instruction_for_trace = instruction.clone();
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
                let bytes = self.kernel.read(&mut self.ctx, state_id)?;
                let mut arr = [0u8; 8];
                let len = bytes.len().min(8);
                arr[..len].copy_from_slice(&bytes[..len]);
                let val = u64::from_le_bytes(arr);
                self.registers.set(reg, RegisterValue::U64(val));
            }
            Instruction::LoadStateBytes { reg, state_id } => {
                let bytes = self.kernel.read(&mut self.ctx, state_id)?;
                self.registers.set(reg, RegisterValue::Bytes(bytes));
            }
            Instruction::WriteRegister { state_id, reg } => {
                let payload = match self.registers.get(reg) {
                    RegisterValue::U64(v) => v.to_le_bytes().to_vec(),
                    RegisterValue::Bytes(b) => b.clone(),
                    RegisterValue::Empty => vec![],
                };
                if let Some(&cap_id) = self.execution.capability_ids.first() {
                    self.ctx.capabilities.push(cap_id);
                }
                self.kernel
                    .write(&mut self.ctx, state_id, payload.clone())?;
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
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
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
                if self.flags.zero {
                    self.pc = target;
                } else {
                    self.pc += consumed;
                }
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Jnz { target } => {
                if !self.flags.zero {
                    self.pc = target;
                } else {
                    self.pc += consumed;
                }
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Jn { target } => {
                if self.flags.negative {
                    self.pc = target;
                } else {
                    self.pc += consumed;
                }
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Call {
                object_id,
                entry_pc,
            } => {
                // P3: CALL enters AccessIntent path — same authorize entry as
                // Read/Write/Link/... (commit-time verify_capability).
                let object_id = self.resolve_operand(&object_id);
                let intent = crate::types::AccessIntent::Call(object_id);
                if let Err(_) = self.kernel.engine().authorize_intent(&self.ctx, &intent) {
                    let reason = crate::types::TrapReason::AccessDenied { pc: self.pc };
                    self.trap_frame = Some(crate::types::TrapFrame {
                        pc: self.pc,
                        reason: reason.clone(),
                        cycles: 0,
                    });
                    self.status = MachineStatus::Trapped(reason);
                    return Ok(());
                }
                // Record for commit-time AccessIntent coverage (self-call is
                // exempt inside authorize_intent but still harmless to record).
                if object_id != self.ctx.current_object && object_id != self.ctx.capability_context
                {
                    self.ctx.pending_calls.push(object_id);
                }

                let return_pc = self.pc + consumed;
                let saved_object = self.ctx.current_object;
                self.call_stack.push(CallFrame {
                    return_pc,
                    parent_object: saved_object,
                    registers: self.registers.clone(),
                    caller_capability_context: self.ctx.capability_context,
                });
                self.ctx.capability_context = object_id;
                self.ctx.current_object = object_id;
                self.pc = entry_pc;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx.capability_context = frame.caller_capability_context;
                        self.ctx.current_object = frame.parent_object;
                        self.registers = frame.registers;
                        self.pc = frame.return_pc;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Trap { service_id } => {
                // P28: TRAP unified kernel service call
                // Decode via KernelCall::decode_with_memory(), dispatch to Kernel::handle()
                // Post-handle Machine glue (not Kernel semantics):
                //   - ObjectBirth: attach pending self-AdminCap (Machine post-handle glue for TRAP ObjectBirth)
                //   - Abort: set MachineStatus::Aborted (Machine lifecycle)
                {
                    let r0 = self.registers.get_u64(0);
                    let r1 = self.registers.get_u64(1);
                    let r2 = self.registers.get_u64(2);

                    let call = match crate::kernel::KernelCall::decode_with_memory(
                        service_id, r0, r1, r2, &self.ram,
                    ) {
                        Ok(call) => call,
                        Err(_) => {
                            // Malformed ABI / unknown service → InvalidEncoding (fail-closed)
                            self.status =
                                MachineStatus::Trapped(crate::types::TrapReason::InvalidEncoding {
                                    pc: self.pc,
                                });
                            return Ok(());
                        }
                    };

                    // Capture Abort reason before move into handle
                    let abort_reason = match &call {
                        crate::kernel::KernelCall::Abort { reason } => Some(*reason),
                        _ => None,
                    };

                    let result = self.kernel.handle(&mut self.ctx, call);

                    if let crate::kernel::TrapResult::Error(code) = result {
                        let reason = map_trap_code(code, self.pc);
                        self.trap_frame = Some(crate::types::TrapFrame {
                            pc: self.pc,
                            reason: reason.clone(),
                            cycles: 0,
                        });
                        self.status = MachineStatus::Trapped(reason);
                        return Ok(());
                    }

                    // Abort: Kernel marks tx aborted; Machine enters Aborted status
                    if let Some(r) = abort_reason {
                        self.pc += consumed;
                        self.status = MachineStatus::Aborted(r);
                        return Ok(());
                    }

                    // Write result to r0 (EffectKey as UTF-8 bytes per TRAP ABI freeze)
                    match result {
                        crate::kernel::TrapResult::ObjectId(id) => {
                            self.registers.set(0, RegisterValue::U64(id));
                            // TRAP ObjectBirth: attach pending self-AdminCap
                            // so subsequent CALL can pass authorize_intent.
                            if let Some(grant) = self.ctx.pending_capabilities.iter().find(|g| {
                                g.grantee == id && g.resource == id && g.cap_type == "AdminCap"
                            }) {
                                let cap_id = grant.capability_id;
                                self.kernel
                                    .engine()
                                    .attach_capability(&mut self.ctx, cap_id);
                            }
                        }
                        crate::kernel::TrapResult::CapabilityId(id) => {
                            self.registers.set(0, RegisterValue::U64(id));
                        }
                        crate::kernel::TrapResult::StateId(id) => {
                            self.registers.set(0, RegisterValue::U64(id));
                        }
                        crate::kernel::TrapResult::EffectKey(key) => {
                            self.registers
                                .set(0, RegisterValue::Bytes(key.into_bytes()));
                        }
                        crate::kernel::TrapResult::Success => {}
                        crate::kernel::TrapResult::Error(_) => unreachable!(),
                    }
                }
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::HostCall { call_id } => {
                // P27/P30.4: HostCall 统一收口 — 合法 ID 由 host::HostCall 枚举定义
                match crate::host::HostCall::from_id(call_id) {
                    Some(_hc) => {
                        // valid, handled by host (Time/Random/Write/Read/Spawn)
                    }
                    None => {
                        self.status =
                            MachineStatus::Trapped(crate::types::TrapReason::InvalidEncoding {
                                pc: self.pc,
                            });
                        return Ok(());
                    }
                }
                self.pc += consumed;
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Read { state_id } => {
                let state_id = self.resolve_operand(&state_id);
                let bytes = self.kernel.read(&mut self.ctx, state_id)?;
                self.registers.set(0, RegisterValue::Bytes(bytes));
                self.pc += consumed;
                self.record_trace(pc_before, regs_before, &instruction_for_trace, consumed);
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
            Instruction::Write { state_id, payload } => {
                let state_id = self.resolve_operand(&state_id);
                self.kernel
                    .write(&mut self.ctx, state_id, payload.clone())?;
                self.execution.record_write(state_id, payload);
                self.pc += consumed;
                self.record_trace(pc_before, regs_before, &instruction_for_trace, consumed);
                if self.pc >= self.ram.len() {
                    self.status = MachineStatus::Halted;
                }
                return Ok(());
            }
        }

        // P1a: execute_kernel_instruction removed — all kernel ops now via KernelCall

        self.pc += consumed;

        // P19.1: 记录指令执行 trace
        let regs_after = [
            self.registers.get_u64(0),
            self.registers.get_u64(1),
            self.registers.get_u64(2),
            self.registers.get_u64(3),
            self.registers.get_u64(4),
            self.registers.get_u64(5),
            self.registers.get_u64(6),
            self.registers.get_u64(7),
        ];
        self.execution
            .record_instruction(crate::trace::InstructionTrace {
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

    // P1a: execute_kernel_instruction removed — KernelCall is the sole dispatch path

    pub fn run(&mut self) -> Result<(), VeritasError> {
        while self.status == MachineStatus::Ready || self.status == MachineStatus::Running {
            self.step()?;
        }
        Ok(())
    }

    pub fn run_with_config(
        &mut self,
        config: ExecutionConfig,
    ) -> Result<ExecutionResult, VeritasError> {
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
        self.execution = crate::execution::ExecutionContext::new(0, [0u8; 32]);
        Ok(self)
    }

    pub fn boot(&mut self, image: crate::program::ProgramImage) -> Result<(), VeritasError> {
        self.registers.reset();
        self.flags.reset();
        self.ram.clear();
        let prog_hash = image.hash();

        let mut addr = 0usize;
        for inst in &image.instructions {
            let encoded = inst.encode()?;
            self.ram
                .write_bytes(addr, &encoded)
                .map_err(|e| VeritasError::EngineError(e))?;
            addr += encoded.len();
        }

        self.pc = image.entry_point as usize;
        self.status = MachineStatus::Running;
        self.trap_frame = None;
        self.execution =
            crate::execution::ExecutionContext::new(prog_hash, self.kernel.state_root());
        Ok(())
    }

    pub fn boot_bytes(&mut self, bytes: &[u8]) -> Result<(), VeritasError> {
        let image = crate::program::ProgramImage::decode(bytes)?;
        self.boot(image)
    }

    pub fn registers(&self) -> &RegisterFile {
        &self.registers
    }

    pub fn flags(&self) -> &FlagsRegister {
        &self.flags
    }

    pub fn status(&self) -> &MachineStatus {
        &self.status
    }

    pub fn trap_frame(&self) -> Option<&crate::types::TrapFrame> {
        self.trap_frame.as_ref()
    }

    pub fn trace_hash(&self) -> u64 {
        self.execution.trace.trace_hash()
    }

    pub fn execution_receipt(&self) -> crate::receipt::ExecutionReceipt {
        crate::receipt::ReceiptBuilder::build(&self.execution, self.state_root())
    }

    pub fn state_root(&self) -> [u8; 32] {
        self.kernel.state_root()
    }

    pub fn is_halted(&self) -> bool {
        matches!(
            self.status,
            MachineStatus::Halted | MachineStatus::Aborted(_) | MachineStatus::Trapped(_)
        )
    }
}

#[cfg(test)]
mod trap_mapping_tests {
    use super::map_trap_code;
    use crate::kernel::{
        TRAP_ERR_ACCESS_DENIED, TRAP_ERR_ENGINE, TRAP_ERR_MEMORY_FAULT, TRAP_ERR_PERMISSION_DENIED,
        TRAP_ERR_STATE_NOT_FOUND, TRAP_ERR_WRITE_CONFLICT,
    };
    use crate::types::TrapReason;

    #[test]
    fn code_1_maps_to_access_denied() {
        assert_eq!(
            map_trap_code(TRAP_ERR_ACCESS_DENIED, 42),
            TrapReason::AccessDenied { pc: 42 }
        );
    }

    #[test]
    fn code_2_maps_to_engine_error() {
        assert_eq!(
            map_trap_code(TRAP_ERR_ENGINE, 7),
            TrapReason::EngineError { pc: 7 }
        );
    }

    #[test]
    fn code_3_reserved_does_not_fabricate_memory_fault() {
        let reason = map_trap_code(TRAP_ERR_MEMORY_FAULT, 5);
        assert!(
            !matches!(reason, TrapReason::MemoryFault { .. }),
            "reserved MEMORY_FAULT must not fabricate MemoryFault"
        );
        assert_eq!(
            reason,
            TrapReason::UnknownKernelError {
                code: TRAP_ERR_MEMORY_FAULT,
                pc: 5
            }
        );
    }

    #[test]
    fn code_4_maps_to_write_conflict_not_illegal_instruction() {
        let reason = map_trap_code(TRAP_ERR_WRITE_CONFLICT, 10);
        assert!(
            !matches!(reason, TrapReason::IllegalInstruction { .. }),
            "WRITE_CONFLICT must not map to IllegalInstruction"
        );
        assert_eq!(reason, TrapReason::WriteConflict { pc: 10 });
    }

    #[test]
    fn code_5_maps_to_access_denied() {
        assert_eq!(
            map_trap_code(TRAP_ERR_PERMISSION_DENIED, 43),
            TrapReason::AccessDenied { pc: 43 }
        );
    }

    #[test]
    fn code_6_maps_to_state_not_found_not_invalid_encoding() {
        let reason = map_trap_code(TRAP_ERR_STATE_NOT_FOUND, 11);
        assert!(
            !matches!(reason, TrapReason::InvalidEncoding { .. }),
            "STATE_NOT_FOUND must not map to InvalidEncoding"
        );
        assert_eq!(reason, TrapReason::StateNotFound { pc: 11 });
    }

    #[test]
    fn unknown_code_maps_to_unknown_kernel_error() {
        let unknown = 99u8;
        assert_eq!(
            map_trap_code(unknown, 23),
            TrapReason::UnknownKernelError {
                code: unknown,
                pc: 23
            }
        );
    }

    #[test]
    fn code_1_and_code_5_share_trap_reason_but_are_distinct_abi_codes() {
        // code 1: Machine CALL 层级的访问拒绝（预留）
        // code 5: Kernel PermissionDenied
        // 两者都映射到 AccessDenied，但 ABI code 不同，语义层级不同
        let machine_access = map_trap_code(TRAP_ERR_ACCESS_DENIED, 1);
        let kernel_permission = map_trap_code(TRAP_ERR_PERMISSION_DENIED, 1);
        assert_eq!(machine_access, TrapReason::AccessDenied { pc: 1 });
        assert_eq!(kernel_permission, TrapReason::AccessDenied { pc: 1 });
        assert_ne!(TRAP_ERR_ACCESS_DENIED, TRAP_ERR_PERMISSION_DENIED);
    }
}
