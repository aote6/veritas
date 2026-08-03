#![allow(dead_code)]
use std::sync::atomic::{AtomicU64, Ordering};
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::{ObjectId, ObjectType, TransactionContext};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TestKernel {
    pub kernel: Kernel,
    pub root_object: ObjectId,
}

pub fn new_kernel() -> TestKernel {
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let wal_path = format!("target/test_{}_{}.wal", std::process::id(), test_id);

    let _ = std::fs::remove_file(&wal_path);

    let kernel = Kernel::with_wal_path(wal_path);
    let init_data = 0u64.to_le_bytes().to_vec();

    // Phase 1: 诞生 Root Object（Kernel 内部分配 id）
    let mut tx = kernel.begin();
    let root_object = match kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).expect("Failed to birth Root Object") {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).expect("Failed to commit Root Object birth");

    // Phase 2: 初始化 Root Object Memory
    let mut tx_init = kernel.begin_in_object(root_object);
    kernel.write(&mut tx_init, 0, init_data).expect("Failed to init Root Object memory");
    kernel.handle(&mut tx_init, KernelCall::Commit).expect("Failed to commit Root Object initialization");

    TestKernel {
        kernel,
        root_object,
    }
}

impl TestKernel {
    pub fn begin(&self) -> TransactionContext {
        self.kernel.begin_in_object(self.root_object)
    }
}
