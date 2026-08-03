pub mod runtime;
pub mod module;
pub mod memory;
pub mod state_memory;
pub mod machine;
pub mod instruction;
pub mod instruction_codec;
pub mod assembler;
pub mod program;
pub mod verifier;
pub mod executor;
// Veritas Kernel V0.2 - 主入口

pub mod engine;
pub mod kernel;
pub mod types;
pub mod wal;
pub mod view;
pub mod guard;
pub mod lock;
pub mod controller;
pub mod tx_manager;
pub mod scope;
pub mod scope_registry;
pub mod capability;
pub mod effect;
pub mod store;
pub mod extension;

use types::*;
// use engine::VeritasEngine;


#[cfg(test)]
mod integration_tests {
    use crate::assembler::assemble;
    use crate::program::ProgramImage;
        use crate::machine::{Machine, RegisterValue};

    #[test]
    fn test_e2e_asm_to_machine() {
        let src = "
            LOAD_CONST R0, 10
            LOAD_CONST R1, 20
            ADD R2, R0, R1
            HALT
        ";
        let insts = assemble(src).unwrap();
        let image = ProgramImage::new(insts);
        let bytes = image.encode().unwrap();

        let kernel = std::sync::Arc::new(crate::kernel::Kernel::new());
        let mut machine = Machine::new(std::sync::Arc::clone(&kernel));
        machine.boot_bytes(&bytes).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(2), &RegisterValue::U64(30));
        assert!(machine.is_halted());
    }

    #[test]
    fn test_e2e_loop_sum() {
        let src = "
            LOAD_CONST R0, 0
            LOAD_CONST R1, 5
            LOAD_CONST R2, 1
            LOAD_CONST R3, 0
        loop:
            ADD R0, R0, R1
            SUB R1, R1, R2
            CMP R1, R3
            JNZ loop
            HALT
        ";
        let insts = assemble(src).unwrap();
        let image = ProgramImage::new(insts);
        let bytes = image.encode().unwrap();

        let kernel = std::sync::Arc::new(crate::kernel::Kernel::new());
        let mut machine = Machine::new(std::sync::Arc::clone(&kernel));
        machine.boot_bytes(&bytes).unwrap();
        machine.run().unwrap();

        assert_eq!(machine.registers().get(0), &RegisterValue::U64(15));
        assert!(machine.is_halted());
    }

    #[test]
    fn test_e2e_corrupted_image_rejected() {
        let src = "LOAD_CONST R0, 1
HALT";
        let insts = assemble(src).unwrap();
        let image = ProgramImage::new(insts);
        let mut bytes = image.encode().unwrap();
        bytes[20] ^= 0xFF;

        let kernel = std::sync::Arc::new(crate::kernel::Kernel::new());
        let mut machine = Machine::new(std::sync::Arc::clone(&kernel));
        assert!(machine.boot_bytes(&bytes).is_err());
    }

    #[test]
    fn test_e2e_trap_invalid_opcode() {
        // 只在 RAM 中放入一个非法 opcode
        let kernel = std::sync::Arc::new(crate::kernel::Kernel::new());
        let mut machine = Machine::new(std::sync::Arc::clone(&kernel));
        machine.ram_mut().write_bytes(0, &[0xEE]).unwrap();
        machine.set_pc(0);
        machine.step().unwrap();
        assert!(matches!(machine.status(), crate::machine::MachineStatus::Trapped(_)));

    }
}
pub mod checkpoint;
pub mod history;
pub mod replay;
pub mod trace;
pub mod execution;
pub mod event;
pub mod receipt;
pub mod replay_verify;

pub mod graph;
