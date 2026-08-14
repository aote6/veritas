use crate::kernel::Kernel;
use crate::machine::Machine;
use crate::module::ModuleImage;
use crate::types::VeritasError;
use std::sync::Arc;

pub struct Runtime;

#[derive(Debug, Clone)]
pub enum ExecutionOutcome {
    Completed {
        pc: usize,
        r0: u64,
    },
    Trapped {
        pc: usize,
        reason: crate::types::TrapReason,
        r0: u64,
    },
}

impl Runtime {
    /// Execute a module on an existing Kernel world.
    /// The Kernel persists beyond this call — objects created by this module
    /// remain alive for subsequent module executions on the same Kernel.
    pub fn execute(
        kernel: &Arc<Kernel>,
        module: &ModuleImage,
    ) -> Result<ExecutionOutcome, VeritasError> {
        let mut machine = Machine::new(Arc::clone(kernel));
        machine.boot(module.program_image.clone())?;

        while !machine.is_halted() {
            machine.step()?;
        }

        let pc = machine.pc();
        let r0 = machine.registers().get_u64(0);

        match machine.trap_frame() {
            Some(frame) => Ok(ExecutionOutcome::Trapped {
                pc: frame.pc,
                reason: frame.reason.clone(),
                r0,
            }),
            None => Ok(ExecutionOutcome::Completed { pc, r0 }),
        }
    }
}
