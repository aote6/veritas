//! P2: CapabilityRevoke Kernel → Engine → Graph → WAL/Checkpoint closure.

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.test_begin();
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

/// Birth an object under `creator` so creator receives AdminCap on the newborn.
fn birth_under(kernel: &Kernel, creator: u64) -> u64 {
    let mut tx = kernel.test_begin_in_object(creator);
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

fn delegate(kernel: &Kernel, cap: u64, from: u64, to: u64, cascade: bool) {
    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityDelegate {
                capability_id: cap,
                from,
                to,
                cascade_on_revoke: cascade,
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

/// STRICT: grantor must hold active AdminCap(resource).
/// Callers must birth resource under grantor (via birth_under).
fn grant(kernel: &Kernel, grantor: u64, grantee: u64, resource: u64) -> u64 {
    let mut tx = kernel.test_begin_in_object(grantor);
    let cap_id = match kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantor,
                grantee,
                capability_type: "read".into(),
                resource,
            },
        )
        .unwrap()
    {
        TrapResult::CapabilityId(id) => id,
        _ => panic!("expected CapabilityId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    cap_id
}

/// Test 1: cascade revoke of intermediate holder removes downstream.
#[test]
fn kernel_capability_revoke_cascade_downstream() {
    let wal = format!(
        "{}/test_cap_revoke_cascade_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal);
    let kernel = Kernel::with_wal_path(wal);

    let o1 = birth(&kernel);
    let o2 = birth(&kernel);
    let o3 = birth(&kernel);
    let resource = birth_under(&kernel, o1);

    let cap = grant(&kernel, o1, o1, resource);
    assert!(kernel.test_engine().holds_capability(cap, o1));

    delegate(&kernel, cap, o1, o2, true);
    delegate(&kernel, cap, o2, o3, true);
    assert!(kernel.test_engine().holds_capability(cap, o2));
    assert!(kernel.test_engine().holds_capability(cap, o3));

    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder: o2,
                cascade_override: Some(true),
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    assert!(kernel.test_engine().holds_capability(cap, o1));
    assert!(!kernel.test_engine().holds_capability(cap, o2));
    assert!(!kernel.test_engine().holds_capability(cap, o3));
}

/// Test 2: non-cascade revoke keeps downstream active.
#[test]
fn kernel_capability_revoke_non_cascade_preserves_downstream() {
    let wal = format!(
        "{}/test_cap_revoke_noncascade_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal);
    let kernel = Kernel::with_wal_path(wal);

    let o1 = birth(&kernel);
    let o2 = birth(&kernel);
    let o3 = birth(&kernel);
    let resource = birth_under(&kernel, o1);

    let cap = grant(&kernel, o1, o1, resource);
    delegate(&kernel, cap, o1, o2, false);
    delegate(&kernel, cap, o2, o3, true);

    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder: o2,
                cascade_override: None, // edge o1→o2 was cascade=false
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    assert!(kernel.test_engine().holds_capability(cap, o1));
    assert!(!kernel.test_engine().holds_capability(cap, o2));
    assert!(
        kernel.test_engine().holds_capability(cap, o3),
        "non-cascade must preserve downstream holder"
    );
}

/// Test 3: revoke result is visible in checkpoint restore.
#[test]
fn kernel_capability_revoke_survives_checkpoint() {
    let wal = format!(
        "{}/test_cap_revoke_ckpt_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal);
    let kernel = Kernel::with_wal_path(wal);

    let o1 = birth(&kernel);
    let o2 = birth(&kernel);
    let resource = birth_under(&kernel, o1);
    let cap = grant(&kernel, o1, o1, resource);
    delegate(&kernel, cap, o1, o2, true);

    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder: o1,
                cascade_override: Some(true),
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    assert!(!kernel.test_engine().holds_capability(cap, o1));
    assert!(!kernel.test_engine().holds_capability(cap, o2));

    let snap = kernel.create_checkpoint();
    let wal2 = format!(
        "{}/test_cap_revoke_ckpt2_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal2);
    let kernel2 = Kernel::with_wal_path(wal2);
    assert!(kernel2.restore_checkpoint(&snap));

    assert!(!kernel2.test_engine().holds_capability(cap, o1));
    assert!(!kernel2.test_engine().holds_capability(cap, o2));
}

/// Test 4: WAL recovery re-applies CapabilityRevoke (root grant only, WAL-recorded).
#[test]
fn kernel_capability_revoke_wal_replay() {
    let wal = format!(
        "{}/test_cap_revoke_wal_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal);
    let kernel = Kernel::with_wal_path(wal.clone());

    let o1 = birth(&kernel);
    let resource = birth_under(&kernel, o1);
    let cap = grant(&kernel, o1, o1, resource);
    assert!(kernel.test_engine().holds_capability(cap, o1));

    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder: o1,
                cascade_override: Some(true),
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    assert!(!kernel.test_engine().holds_capability(cap, o1));

    // Fresh engine recovers WAL (grants then revokes)
    let kernel2 = Kernel::with_wal_path(wal);
    assert!(
        !kernel2.test_engine().holds_capability(cap, o1),
        "CapabilityRevoke must be durable across WAL recovery"
    );
}

/// Revoke of non-holder fails before commit.
#[test]
fn kernel_capability_revoke_not_holder_errors() {
    let wal = format!(
        "{}/test_cap_revoke_err_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal);
    let kernel = Kernel::with_wal_path(wal);

    let o1 = birth(&kernel);
    let o2 = birth(&kernel);
    let resource = birth_under(&kernel, o1);
    let cap = grant(&kernel, o1, o1, resource);

    let mut tx = kernel.test_begin();
    let err = kernel.handle(
        &mut tx,
        KernelCall::CapabilityRevoke {
            capability_id: cap,
            holder: o2,
            cascade_override: None,
        },
    );
    assert!(err.is_err());
}
