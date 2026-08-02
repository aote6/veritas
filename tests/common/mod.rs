#![allow(dead_code)]
use std::sync::atomic::{AtomicU64, Ordering};
use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectId, StateId, TransactionContext};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TestKernel {
    pub engine: VeritasEngine,
    pub root_object: ObjectId,
}

/// 确定性根对象生成算法，避免 ObjectId = 0 魔数冲突
pub fn root_object_id() -> ObjectId {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset
    for byte in b"veritas::kernel::root_object" {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash
}

pub fn new_kernel() -> TestKernel {
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let wal_path = format!("target/test_{}_{}.wal", std::process::id(), test_id);

    let _ = std::fs::remove_file(&wal_path);

    let engine = VeritasEngine::with_wal_path(wal_path);
    let root_object = root_object_id();

    // Phase 1: 宪法底座 - 诞生 Root Object 并提交
    let mut tx_birth = engine.begin_in_object(root_object);
    engine.object_birth(&mut tx_birth, root_object).expect("Failed to birth Root Object");
    engine.commit(&mut tx_birth).expect("Failed to commit Root Object birth");

    // Phase 2: 宪法底座 - 初始化 Root Object Memory
    let mut tx_init = engine.begin_in_object(root_object);
    let init_data = 0u64.to_le_bytes().to_vec();
    engine.write(&mut tx_init, 0, init_data).expect("Failed to init Root Object memory");
    engine.commit(&mut tx_init).expect("Failed to commit Root Object initialization");

    TestKernel {
        engine,
        root_object,
    }
}

impl TestKernel {
    pub fn begin(&self) -> TransactionContext {
        self.engine.begin_in_object(self.root_object)
    }

    #[allow(dead_code)]
    pub fn init_state(&self, state_id: StateId, value: Vec<u8>) {
        self.engine.init_state(state_id, value);
    }
}
