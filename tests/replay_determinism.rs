use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::ObjectType;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.begin();
    let id = match kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).unwrap() {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

fn wal_path(prefix: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("target/test_det_{}_{}_{}.wal", std::process::id(), prefix, n)
}

/// P30.1: Same WAL → identical Engine state
#[test]
fn same_wal_same_state() {
    let path = wal_path("same");
    let _ = std::fs::remove_file(&path);

    let a: u64;
    let b: u64;
    {
        let kernel = Kernel::with_wal_path(path.clone());
        a = birth(&kernel);
        b = birth(&kernel);
    }

    {
        let kernel = Kernel::with_wal_path(path.clone());
        assert_eq!(kernel.engine().get_object_state(a), Some(veritas_kernel::types::ObjectState::Alive));
        assert_eq!(kernel.engine().get_object_state(b), Some(veritas_kernel::types::ObjectState::Alive));
    }

    let _ = std::fs::remove_file(&path);
}

/// P30.1: WAL reconstruction must be deterministic
#[test]
fn replay_is_deterministic() {
    let path = wal_path("det");
    let _ = std::fs::remove_file(&path);

    {
        let kernel = Kernel::with_wal_path(path.clone());
        birth(&kernel);
        birth(&kernel);
    }

    let ids1: Vec<u64>;
    let ids2: Vec<u64>;
    {
        let kernel = Kernel::with_wal_path(path.clone());
        ids1 = { let mut ids = kernel.engine().list_object_ids(); ids.sort(); ids };
    }
    {
        let kernel = Kernel::with_wal_path(path.clone());
        ids2 = { let mut ids = kernel.engine().list_object_ids(); ids.sort(); ids };
    }

    assert_eq!(ids1, ids2, "same WAL must produce identical object lists");
}

/// P30.1: WAL with object operations must replay deterministically
#[test]
fn object_ops_are_deterministic() {
    let path = wal_path("objops");
    let _ = std::fs::remove_file(&path);

    {
        let kernel = Kernel::with_wal_path(path.clone());
        let a = birth(&kernel);
        let b = birth(&kernel);
        let mut tx = kernel.begin_in_object(a);
        kernel.handle(&mut tx, KernelCall::CapabilityGrant {
            grantee: a, capability_type: "link".to_string(), resource: b,
        }).unwrap();
        kernel.handle(&mut tx, KernelCall::ObjectLink { from: a, to: b, link_type: veritas_kernel::types::LinkType::Owns }).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }

    let state1;
    let state2;
    {
        let kernel = Kernel::with_wal_path(path.clone());
        state1 = kernel.engine().state_root();
    }
    {
        let kernel = Kernel::with_wal_path(path.clone());
        state2 = kernel.engine().state_root();
    }

    assert_eq!(state1, state2, "same WAL must produce identical state_root");
}

/// P30.2: WAL contains full world state — objects and topology
#[test]
fn wal_contains_full_world() {
    let path = wal_path("full");
    let _ = std::fs::remove_file(&path);

    let a: u64;
    let b: u64;
    {
        let kernel = Kernel::with_wal_path(path.clone());
        a = birth(&kernel);
        b = birth(&kernel);
        let mut tx = kernel.begin_in_object(a);
        kernel.handle(&mut tx, KernelCall::CapabilityGrant {
            grantee: a, capability_type: "link".to_string(), resource: b,
        }).unwrap();
        kernel.handle(&mut tx, KernelCall::ObjectLink { from: a, to: b, link_type: veritas_kernel::types::LinkType::Owns }).unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    }

    {
        let kernel = Kernel::with_wal_path(path.clone());
        assert!(kernel.engine().has_link(a, b), "link must survive recovery");
        assert_eq!(kernel.engine().get_object_state(a), Some(veritas_kernel::types::ObjectState::Alive));
        assert_eq!(kernel.engine().get_object_state(b), Some(veritas_kernel::types::ObjectState::Alive));
    }

    let _ = std::fs::remove_file(&path);
}
