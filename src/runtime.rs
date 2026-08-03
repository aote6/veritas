use crate::kernel::Kernel;
use crate::machine::Machine;
use crate::module::ModuleImage;
use crate::types::VeritasError;
use std::sync::Arc;

pub struct Runtime;

impl Runtime {
    /// Execute a module on an existing Kernel world.
    /// The Kernel persists beyond this call — objects created by this module
    /// remain alive for subsequent module executions on the same Kernel.
    pub fn execute(
        kernel: &Arc<Kernel>,
        module: &ModuleImage,
    ) -> Result<(usize, u64), VeritasError> {
        let mut machine = Machine::new(Arc::clone(kernel));
        machine.boot(module.program_image.clone())?;

        while !machine.is_halted() {
            machine.step()?;
        }

        let r0 = machine.registers().get_u64(0);
        Ok((machine.pc(), r0))
    }
}
