pub mod memory;
pub mod machine;
pub mod instruction;
pub mod instruction_codec;
pub mod assembler;
pub mod program;
pub mod verifier;
pub mod executor;
// Veritas Kernel V0.2 - 主入口

pub mod engine;
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
use engine::VeritasEngine;


