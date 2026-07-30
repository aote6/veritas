use crate::engine::VeritasEngine;
use crate::machine::Machine;
use crate::module::ModuleImage;
use crate::types::VeritasError;

pub struct Runtime;

impl Runtime {
    pub fn execute(module: &ModuleImage) -> Result<(usize, u64), VeritasError> {
        let engine = VeritasEngine::new();
        let mut machine = Machine::new(&engine);
        machine.boot(module.program_image.clone())?;

        while !machine.is_halted() {
            machine.step()?;
        }

        let r0 = machine.registers().get_u64(0);
        Ok((machine.pc(), r0))
    }
}
