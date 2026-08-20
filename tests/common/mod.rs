#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
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

    let mut tx = kernel.test_begin();
    let root_object = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };

    kernel
        .handle(&mut tx, KernelCall::Commit);

    let mut tx_init = kernel.test_begin_in_object(root_object);
    kernel
        .test_write(&mut tx_init, 0, 0u64.to_le_bytes().to_vec())
        .expect("Failed to init Root Object memory");

    kernel
        .handle(&mut tx_init, KernelCall::Commit);

    TestKernel {
        kernel,
        root_object,
    }
}

impl TestKernel {
    pub fn begin(&self) -> TransactionContext {
        self.kernel.test_begin_in_object(self.root_object)
    }
}

/// Shared test-world fixture.
///
/// This layer constructs legal worlds for integration tests.
/// It must never silently grant capabilities as a side effect
/// of unrelated operations.
pub struct TestWorld {
    pub tk: TestKernel,
}

impl TestWorld {
    pub fn new() -> Self {
        Self { tk: new_kernel() }
    }

    pub fn kernel(&self) -> &Kernel {
        &self.tk.kernel
    }

    pub fn root(&self) -> ObjectId {
        self.tk.root_object
    }

    /// Birth an object directly from the test root.
    pub fn birth(&self) -> ObjectId {
        self.birth_under(self.root())
    }

    /// Birth an object under `creator`.
    ///
    /// The kernel's normal OBJECT_BIRTH semantics establish the
    /// creator -> newborn AdminCap relationship.
    pub fn birth_under(&self, creator: ObjectId) -> ObjectId {
        let mut tx = self.kernel().test_begin_in_object(creator);

        let id = match self
            .kernel()
            .handle(
                &mut tx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )
        {
            TrapResult::ObjectId(id) => id,
            _ => panic!("expected ObjectId"),
        };

        self.kernel()
            .handle(&mut tx, KernelCall::Commit);

        id
    }

    /// Explicitly grant a capability.
    ///
    /// Authorization is intentionally visible to the test.
    /// This helper never runs implicitly from link/write/etc.
    pub fn grant_cap(
        &self,
        grantor: ObjectId,
        grantee: ObjectId,
        capability_type: &str,
        resource: ObjectId,
    ) {
        let mut tx = self.kernel().test_begin_in_object(grantor);

        self.kernel()
            .handle(
                &mut tx,
                KernelCall::CapabilityGrant {
                    grantor,
                    grantee,
                    capability_type: capability_type.to_string(),
                    resource,
                },
            );

        self.kernel()
            .handle(&mut tx, KernelCall::Commit);
    }
}
