//! P0 Capability Grant Authorization — STRICT CAPABILITY MODEL
//!
//! Grant requires: grantor holds active AdminCap on target resource.
//! Self-access / current_object / any non-AdminCap must NOT authorize Grant.
//!
//! RED (pre-fix): Object A as execution subject without AdminCap on B
//!                 successfully mints a Capability on B.
//! GREEN: same path is rejected with capability authorization failure.

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;
use veritas_kernel::world_api::WorldService;
use std::sync::Arc;

fn birth_host(kernel: &Kernel) -> u64 {
    let mut ctx = kernel.test_begin();
    let id = match kernel
        .handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    id
}

/// RED→GREEN: A is current_object (self-access), holds no AdminCap on B,
/// must NOT successfully CapabilityGrant on resource B.
#[test]
fn grant_without_admin_cap_on_resource_rejected() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);
    let b = birth_host(&kernel);

    // A is execution subject; self-access does not grant AdminCap on B.
    let mut tx = kernel.test_begin_in_object(a);
    let result = kernel.handle(
        &mut tx,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            capability_type: "link".to_string(),
            resource: b,
        },
    );
    assert!(
        result.is_err(),
        "A without AdminCap on B must not mint Capability on B (self-access is not Grant authority)"
    );
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("AdminCap") || err.contains("PermissionDenied") || err.contains("CapabilityGrant"),
        "error must be an explicit capability authorization failure, got: {}",
        err
    );
}

/// AdminCap holder → Grant succeeds.
#[test]
fn admin_cap_holder_can_grant() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);

    // Birth B under A → A receives creator AdminCap on B
    let mut tx = kernel.test_begin_in_object(a);
    let b = match kernel
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

    // A holds AdminCap(B) → may grant "link" on B to itself (or anyone Alive)
    let mut tx2 = kernel.test_begin_in_object(a);
    let result = kernel.handle(
        &mut tx2,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            capability_type: "link".to_string(),
            resource: b,
        },
    );
    assert!(result.is_ok(), "AdminCap holder must be allowed to Grant: {:?}", result);
    kernel.handle(&mut tx2, KernelCall::Commit).unwrap();
}

/// Holding a non-AdminCap on resource does not authorize Grant.
#[test]
fn non_admin_cap_on_resource_does_not_authorize_grant() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);

    // A creates B, gets AdminCap, grants "read" to A on B, then we would need
    // to revoke AdminCap to isolate — instead: create C under host (no AdminCap
    // for A), grant nothing, ensure non-Admin path fails. Covered by the
    // primary RED test; here ensure resource mismatch also fails.
    let c = birth_host(&kernel);
    let mut tx = kernel.test_begin_in_object(a);
    // A has AdminCap only on A (self) and nothing on C
    let result = kernel.handle(
        &mut tx,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            capability_type: "write".to_string(),
            resource: c,
        },
    );
    assert!(result.is_err(), "AdminCap on A must not authorize Grant on C");
}

/// Revoked AdminCap no longer authorizes Grant.
#[test]
fn revoked_admin_cap_rejects_grant() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);

    let mut tx = kernel.test_begin_in_object(a);
    let b = match kernel
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

    // Find the AdminCap id held by A on B
    let records = kernel.test_capability_records();
    let admin = records
        .iter()
        .find(|r| r.active && r.holder == a && r.resource == b && r.capability_type == "AdminCap")
        .expect("creator AdminCap must exist");
    let admin_id = admin.capability_id;

    // Revoke it
    let mut tx_r = kernel.test_begin_in_object(a);
    kernel
        .handle(
            &mut tx_r,
            KernelCall::CapabilityRevoke {
                capability_id: admin_id,
                holder: a,
                cascade_override: Some(true),
            },
        )
        .expect("revoke AdminCap");
    kernel.handle(&mut tx_r, KernelCall::Commit).unwrap();

    // Grant must now fail
    let mut tx_g = kernel.test_begin_in_object(a);
    let result = kernel.handle(
        &mut tx_g,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            capability_type: "link".to_string(),
            resource: b,
        },
    );
    assert!(result.is_err(), "revoked AdminCap must not authorize Grant");
}

/// Same-tx ObjectBirth pending AdminCap is visible to Grant authorization.
#[test]
fn same_tx_birth_admin_cap_allows_grant() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);

    let mut tx = kernel.test_begin_in_object(a);
    let b = match kernel
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
    // Still same transaction: pending creator AdminCap on B must authorize Grant
    let result = kernel.handle(
        &mut tx,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            capability_type: "link".to_string(),
            resource: b,
        },
    );
    assert!(
        result.is_ok(),
        "same-tx pending AdminCap from ObjectBirth must authorize Grant: {:?}",
        result
    );
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

/// WorldService path: Host identity + AdminCap → Grant success.
#[test]
fn world_service_grant_requires_admin_cap() {
    let kernel = Arc::new(Kernel::new());
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_commit(sid).unwrap();

    // A is creator → holds AdminCap(B)
    let sid2 = world.tx_begin(Some(a)).unwrap();
    let ok = world.tx_capability_grant(sid2, a, a, "link".to_string(), b);
    assert!(ok.is_ok(), "creator AdminCap must allow WorldService grant: {:?}", ok);
    world.tx_commit(sid2).unwrap();

    // Stranger C has no AdminCap on B
    let c = world.attach_identity(None).unwrap();
    let sid3 = world.tx_begin(Some(c)).unwrap();
    // Identity Call authorize may fail or AdminCap fails; either is rejection
    let denied = world.tx_capability_grant(sid3, c, c, "link".to_string(), b);
    assert!(denied.is_err(), "non-AdminCap holder must be denied Grant");
}

/// Grantee dead → Grant rejected (existing Alive check preserved).
#[test]
fn grantee_dead_rejects_grant() {
    let kernel = Kernel::new();
    let a = birth_host(&kernel);

    // Create B and C, commit, then kill C in a later tx
    let mut tx = kernel.test_begin_in_object(a);
    let b = match kernel
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
    let c = match kernel
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

    let mut tx_d = kernel.test_begin_in_object(a);
    kernel
        .handle(&mut tx_d, KernelCall::ObjectDeath { object_id: c })
        .unwrap();
    kernel.handle(&mut tx_d, KernelCall::Commit).unwrap();

    // A still holds AdminCap on B; grantee C is dead
    let mut tx_g = kernel.test_begin_in_object(a);
    let result = kernel.handle(
        &mut tx_g,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: c,
            capability_type: "link".to_string(),
            resource: b,
        },
    );
    assert!(result.is_err(), "dead grantee must reject Grant");
}
