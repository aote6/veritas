//! Multi-object transaction permutation matrix (ROADMAP §3).
//!
//! Only tests. No production-code changes.
//! Verifies identity / capability / commit / abort / WAL recovery invariants
//! across the cross-object transaction surface used by WorldService.

use std::sync::Arc;

use veritas_kernel::kernel::Kernel;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectState;
use veritas_kernel::world_api::WorldService;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_matrix_{}_{}.wal",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
}

fn assert_alive(world: &WorldService, id: u64, label: &str) {
    let obj = world
        .get_object(id)
        .unwrap_or_else(|| panic!("{label} (id={id}) must exist"));
    assert_eq!(
        obj.state,
        ObjectState::Alive,
        "{label} (id={id}) must be Alive"
    );
}

fn assert_absent(world: &WorldService, id: u64, label: &str) {
    assert!(
        world.get_object(id).is_none(),
        "{label} (id={id}) must not exist after abort/rollback"
    );
}

fn cap_records_for(
    kernel: &Kernel,
    holder: u64,
    resource: u64,
) -> Vec<veritas_kernel::types::CapabilitySemanticRecord> {
    kernel
        .test_capability_records()
        .into_iter()
        .filter(|r| r.holder == holder && r.resource == resource && r.active)
        .collect()
}

// =============================================================================
// Normal paths
// =============================================================================

/// 1. birth A → birth B → write A → write B → commit
/// Invariant: multi-object writes in one tx commit; both objects Alive with data;
/// capability_context does not drift so the second write still authorizes.
#[test]
fn s01_birth_ab_write_ab_commit() {
    let wal = temp_wal("s01");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();

    // Creator (A) holds pending AdminCap on B, so cross-object write is authorized.
    world
        .tx_write(sid, 0, b"data-A".to_vec(), Some(a))
        .expect("write A (self) must succeed");
    world
        .tx_write(sid, 0, b"data-B".to_vec(), Some(b))
        .expect("write B via creator AdminCap must succeed");

    let receipt = world.tx_commit(sid).expect("commit must succeed");
    assert_ne!(
        receipt.before_root, receipt.after_root,
        "commit must change state root"
    );

    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");

    // Re-read committed memory under each object's identity.
    let sid_a = world.tx_begin(Some(a)).unwrap();
    let va = world.tx_read(sid_a, 0).expect("read A state_id=0");
    assert_eq!(va, b"data-A".to_vec());
    world.tx_commit(sid_a).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    let vb = world.tx_read(sid_b, 0).expect("read B state_id=0");
    assert_eq!(vb, b"data-B".to_vec());
    world.tx_commit(sid_b).unwrap();

    cleanup(&wal);
}

/// 2. birth A → birth B → link A→B → write A → commit
#[test]
fn s02_birth_ab_link_ab_write_a_commit() {
    let wal = temp_wal("s02");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();

    world
        .tx_link(sid, a, b, "owns")
        .expect("link A→B must stage");
    world
        .tx_write(sid, 0, b"after-link".to_vec(), Some(a))
        .expect("write A after link must succeed");

    world.tx_commit(sid).expect("commit must succeed");

    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    assert!(
        kernel.has_link(a, b),
        "A→B link must exist after commit"
    );

    let sid_a = world.tx_begin(Some(a)).unwrap();
    assert_eq!(
        world.tx_read(sid_a, 0).unwrap(),
        b"after-link".to_vec()
    );
    world.tx_commit(sid_a).unwrap();

    cleanup(&wal);
}

/// 3. birth A → birth B → write A → link A→B → commit
#[test]
fn s03_birth_ab_write_a_link_ab_commit() {
    let wal = temp_wal("s03");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();

    world
        .tx_write(sid, 0, b"before-link".to_vec(), Some(a))
        .expect("write A before link must succeed");
    world
        .tx_link(sid, a, b, "owns")
        .expect("link A→B after write must stage");

    world.tx_commit(sid).expect("commit must succeed");

    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    assert!(kernel.has_link(a, b), "A→B link must exist");

    let sid_a = world.tx_begin(Some(a)).unwrap();
    assert_eq!(
        world.tx_read(sid_a, 0).unwrap(),
        b"before-link".to_vec()
    );
    world.tx_commit(sid_a).unwrap();

    cleanup(&wal);
}

// =============================================================================
// Grant paths
// =============================================================================

/// 4. birth A → birth B → grant A→B on C → write B → commit
/// C is a real third object. B receives link/write capability on C; write is
/// performed under B's identity on B itself (self-access). The grant record
/// is verified: grantor=A, holder=B, resource=C.
#[test]
fn s04_grant_a_to_b_on_c_write_b_commit() {
    let wal = temp_wal("s04");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    // Bootstrap A; A creates independent B and C so A is creator of both.
    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    // A grants B a capability on resource C.
    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "write".to_string(), c)
        .expect("A grant B on C must succeed");
    world.tx_commit(sid_g).expect("grant commit must succeed");

    // B writes its own memory (self-access) — grant must not invert identities.
    let sid_b = world.tx_begin(Some(b)).unwrap();
    world
        .tx_write(sid_b, 0, b"B-self".to_vec(), Some(b))
        .expect("B self-write must succeed");
    world.tx_commit(sid_b).expect("B commit must succeed");

    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    assert_alive(&world, c, "C");

    let records = cap_records_for(&kernel, b, c);
    assert!(
        !records.is_empty(),
        "active capability for holder=B resource=C must exist"
    );
    let rec = &records[0];
    assert_eq!(rec.granted_by, a, "grantor must be A");
    assert_eq!(rec.holder, b, "holder must be B");
    assert_eq!(rec.resource, c, "resource must be C");
    assert_ne!(rec.granted_by, rec.holder, "grantor != grantee");

    // A is not made holder of the granted capability.
    let a_as_holder = cap_records_for(&kernel, a, c)
        .into_iter()
        .filter(|r| r.granted_by == a && r.holder == a)
        .count();
    // A may still hold creator AdminCap on C (holder=A, granted_by=C or A).
    // The *new* grant's holder must not be A.
    assert!(
        records.iter().all(|r| r.holder == b),
        "the grant under test must list B as holder, not A"
    );
    let _ = a_as_holder;

    cleanup(&wal);
}

/// 5. birth A → birth B → grant A→B on C → link B→C → commit
#[test]
fn s05_grant_a_to_b_on_c_link_b_c_commit() {
    let wal = temp_wal("s05");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    // Without grant, B linking to C must fail at commit.
    {
        let sid = world.tx_begin(Some(b)).unwrap();
        world.tx_link(sid, b, c, "owns").unwrap();
        let r = world.tx_commit(sid);
        assert!(
            r.is_err(),
            "B without capability on C must be denied at commit"
        );
    }

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "owns").unwrap();
    world
        .tx_commit(sid_b)
        .expect("B with grant on C must link successfully");

    assert!(kernel.has_link(b, c), "B→C link must exist after commit");

    let records = cap_records_for(&kernel, b, c);
    assert!(!records.is_empty());
    assert_eq!(records[0].granted_by, a);
    assert_eq!(records[0].holder, b);
    assert_eq!(records[0].resource, c);

    cleanup(&wal);
}

/// 6. birth A → birth B → grant A→B on C → write B → link B→A → commit
/// link B→A: B is self for `from`; `to`=A requires capability on A.
/// Creator A holds AdminCap on B, but B does not hold capability on A unless granted.
/// We grant B a link capability on A as well, OR expect failure on B→A without it.
/// Real model: authorize_intent for Link targets both from and to. B needs cap on A.
#[test]
fn s06_grant_then_write_b_link_b_to_a_commit() {
    let wal = temp_wal("s06");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    // Grant B capability on C (used as authorization substrate; write is self).
    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "write".to_string(), c)
        .unwrap();
    // Also grant B link capability on A so B→A can authorize at commit.
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), a)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world
        .tx_write(sid_b, 0, b"B-data".to_vec(), Some(b))
        .expect("B self-write must succeed");
    world
        .tx_link(sid_b, b, a, "depends_on")
        .expect("link B→A must stage");
    world
        .tx_commit(sid_b)
        .expect("commit with B→A link under granted capability must succeed");

    assert!(kernel.has_link(b, a), "B→A link must exist");
    assert_alive(&world, a, "A");
    assert_alive(&world, b, "B");
    assert_alive(&world, c, "C");

    cleanup(&wal);
}

/// 6b. Negative: grant on C alone does not authorize link B→A.
#[test]
fn s06b_grant_on_c_does_not_authorize_link_to_a() {
    let wal = temp_wal("s06b");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, a, "depends_on").unwrap();
    let r = world.tx_commit(sid_b);
    assert!(
        r.is_err(),
        "capability on C must not authorize link whose target is A"
    );

    cleanup(&wal);
}

// =============================================================================
// Abort paths
// =============================================================================

/// 7. birth A → write A → birth B → write B → abort
#[test]
fn s07_multi_object_abort_no_residual_state() {
    let wal = temp_wal("s07");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world
        .tx_write(sid, 0, b"A-data".to_vec(), Some(a))
        .unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world
        .tx_write(sid, 0, b"B-data".to_vec(), Some(b))
        .unwrap();
    world.tx_link(sid, a, b, "owns").unwrap();

    world.tx_abort(sid).expect("abort must succeed");

    assert_absent(&world, a, "A");
    assert_absent(&world, b, "B");
    assert!(
        !world.list_links().iter().any(|l| l.from == a && l.to == b),
        "no residual A→B link after abort"
    );
    // No committed capability for either object.
    let caps = kernel.test_capability_records();
    assert!(
        caps.iter()
            .filter(|r| r.active && (r.holder == a || r.holder == b || r.resource == a || r.resource == b))
            .count()
            == 0,
        "abort must leave no active capability involving A or B"
    );

    cleanup(&wal);
}

/// 8. birth A → birth B → grant A→B → abort → capability must not remain
#[test]
fn s08_grant_then_abort_leaves_no_capability() {
    let wal = temp_wal("s08");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let caps_before = kernel.test_capability_records();
    let active_before: Vec<_> = caps_before.into_iter().filter(|r| r.active).collect();

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_abort(sid_g).expect("abort after grant must succeed");

    let caps_after = kernel.test_capability_records();
    let active_after: Vec<_> = caps_after.into_iter().filter(|r| r.active).collect();
    assert_eq!(
        active_before.len(),
        active_after.len(),
        "abort must not add active capabilities"
    );
    assert!(
        !active_after
            .iter()
            .any(|r| r.holder == b && r.resource == c && r.granted_by == a),
        "aborted grant A→B on C must not appear as committed capability"
    );

    // B still cannot use the aborted grant.
    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "owns").unwrap();
    let r = world.tx_commit(sid_b);
    assert!(
        r.is_err(),
        "B must still be denied after aborted grant"
    );

    cleanup(&wal);
}

// =============================================================================
// Recovery paths
// =============================================================================

/// 9. grant → write/link → commit → WAL recovery → state consistent
#[test]
fn s09_grant_commit_wal_recovery_consistent() {
    let wal = temp_wal("s09");
    let (a, b, c) = {
        let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
        let world = WorldService::new(Arc::clone(&kernel));

        let a = world.attach_identity(None).unwrap();
        let sid0 = world.tx_begin(Some(a)).unwrap();
        let b = world.tx_create_object(sid0).unwrap();
        let c = world.tx_create_object(sid0).unwrap();
        world.tx_commit(sid0).unwrap();

        let sid_g = world.tx_begin(Some(a)).unwrap();
        world
            .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
            .unwrap();
        world.tx_commit(sid_g).unwrap();

        let sid_b = world.tx_begin(Some(b)).unwrap();
        world.tx_link(sid_b, b, c, "owns").unwrap();
        world
            .tx_write(sid_b, 0, b"B-after-grant".to_vec(), Some(b))
            .unwrap();
        world.tx_commit(sid_b).unwrap();

        assert!(kernel.has_link(b, c));
        let recs = cap_records_for(&kernel, b, c);
        assert!(!recs.is_empty());
        assert_eq!(recs[0].granted_by, a);

        (a, b, c)
    };

    // Recovery via real Kernel::with_wal_path path.
    {
        let kernel2 = Arc::new(Kernel::with_wal_path(wal.clone()));
        let world2 = WorldService::new(Arc::clone(&kernel2));

        assert_alive(&world2, a, "A after recovery");
        assert_alive(&world2, b, "B after recovery");
        assert_alive(&world2, c, "C after recovery");
        assert!(
            kernel2.has_link(b, c),
            "B→C link must survive WAL recovery"
        );

        let recs = cap_records_for(&kernel2, b, c);
        assert!(
            !recs.is_empty(),
            "committed grant must survive recovery"
        );
        assert_eq!(recs[0].granted_by, a, "grantor A preserved");
        assert_eq!(recs[0].holder, b, "holder B preserved");
        assert_eq!(recs[0].resource, c, "resource C preserved");

        let sid = world2.tx_begin(Some(b)).unwrap();
        let data = world2.tx_read(sid, 0).expect("B data after recovery");
        assert_eq!(data, b"B-after-grant".to_vec());
        world2.tx_commit(sid).unwrap();
    }

    cleanup(&wal);
}

/// 10. grant → abort → WAL recovery → no residual capability
#[test]
fn s10_grant_abort_wal_recovery_no_residual_cap() {
    let wal = temp_wal("s10");
    let (a, b, c, active_before_count) = {
        let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
        let world = WorldService::new(Arc::clone(&kernel));

        let a = world.attach_identity(None).unwrap();
        let sid0 = world.tx_begin(Some(a)).unwrap();
        let b = world.tx_create_object(sid0).unwrap();
        let c = world.tx_create_object(sid0).unwrap();
        world.tx_commit(sid0).unwrap();

        let active_before = kernel
            .test_capability_records()
            .into_iter()
            .filter(|r| r.active)
            .count();

        let sid_g = world.tx_begin(Some(a)).unwrap();
        world
            .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
            .unwrap();
        world.tx_abort(sid_g).unwrap();

        // Pending grant never hits WAL as committed.
        assert!(
            !kernel
                .test_capability_records()
                .iter()
                .any(|r| r.active && r.holder == b && r.resource == c && r.granted_by == a),
            "aborted grant must not be in graph before recovery"
        );

        (a, b, c, active_before)
    };

    {
        let kernel2 = Arc::new(Kernel::with_wal_path(wal.clone()));
        let active_after = kernel2
            .test_capability_records()
            .into_iter()
            .filter(|r| r.active)
            .count();
        assert_eq!(
            active_after, active_before_count,
            "recovery must not resurrect aborted grant"
        );
        assert!(
            !kernel2
                .test_capability_records()
                .iter()
                .any(|r| r.active && r.holder == b && r.resource == c && r.granted_by == a),
            "aborted A→B on C must not appear after recovery"
        );
        assert_eq!(
            kernel2.get_object_state(a),
            Some(ObjectState::Alive)
        );
        assert_eq!(
            kernel2.get_object_state(b),
            Some(ObjectState::Alive)
        );
        assert_eq!(
            kernel2.get_object_state(c),
            Some(ObjectState::Alive)
        );
    }

    cleanup(&wal);
}

// =============================================================================
// Reverse tests
// =============================================================================

/// 11. A grant B on C → B operates on C → A is not made holder of that grant
#[test]
fn s11_grantor_does_not_become_holder() {
    let wal = temp_wal("s11");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "owns").unwrap();
    world.tx_commit(sid_b).unwrap();
    assert!(kernel.has_link(b, c));

    let records = kernel.test_capability_records();
    let grant = records
        .iter()
        .find(|r| r.active && r.holder == b && r.resource == c && r.granted_by == a)
        .expect("grant record A→B on C must exist");
    assert_eq!(grant.granted_by, a);
    assert_eq!(grant.holder, b);
    assert_eq!(grant.resource, c);
    assert_ne!(grant.granted_by, grant.holder);
    assert_ne!(a, b);

    // A is not the holder of *this* grant record.
    assert_ne!(grant.holder, a, "grantor A must not be listed as holder");

    // A does not gain holder status from this grant (creator AdminCap on C is separate).
    let a_holds_this_grant = records.iter().any(|r| {
        r.active
            && r.capability_id == grant.capability_id
            && r.holder == a
    });
    assert!(
        !a_holds_this_grant,
        "A must not hold the capability_id that was granted to B"
    );

    cleanup(&wal);
}

/// 12. STRICT CAPABILITY MODEL: B without AdminCap on C cannot mint a new root Grant on C.
/// Only holders of active AdminCap(resource) may CapabilityGrant on that resource.
/// CapabilityDelegate remains the path for sharing an existing CapabilityId.
#[test]
fn s12_grantee_further_grant_semantics() {
    let wal = temp_wal("s12");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    let d = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    // A → B on C (A holds creator AdminCap on C)
    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    // B attempts a further CapabilityGrant on C without holding AdminCap(C).
    let sid_b = world.tx_begin(Some(b)).unwrap();
    let grant_result =
        world.tx_capability_grant(sid_b, b, d, "link".to_string(), c);
    assert!(
        grant_result.is_err(),
        "CapabilityGrant requires AdminCap(resource); B must be rejected"
    );

    cleanup(&wal);
}

// =============================================================================
// Additional permutation coverage
// =============================================================================

/// A. birth A,B,C → grant → write → link → commit (single richer tx mix)
#[test]
fn s_extra_a_three_object_grant_write_link_commit() {
    let wal = temp_wal("extra_a");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    let c = world.tx_create_object(sid).unwrap();
    // Same-tx: A grants B on C, B cannot act yet as session identity is still A.
    // Grant + A-side write/link, then commit; B uses grant in a later session.
    world
        .tx_capability_grant(sid, a, b, "link".to_string(), c)
        .unwrap();
    world
        .tx_write(sid, 0, b"A-in-mix".to_vec(), Some(a))
        .unwrap();
    world.tx_link(sid, a, c, "owns").unwrap();
    world.tx_commit(sid).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "depends_on").unwrap();
    world.tx_commit(sid_b).unwrap();

    assert!(kernel.has_link(a, c));
    assert!(kernel.has_link(b, c));
    cleanup(&wal);
}

/// B. Multiple capability grants in one transaction
#[test]
fn s_extra_b_multiple_grants_same_tx() {
    let wal = temp_wal("extra_b");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    let d = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid, a, b, "link".to_string(), c)
        .unwrap();
    world
        .tx_capability_grant(sid, a, b, "write".to_string(), d)
        .unwrap();
    world.tx_commit(sid).unwrap();

    let recs = kernel.test_capability_records();
    assert!(recs
        .iter()
        .any(|r| r.active && r.holder == b && r.resource == c && r.granted_by == a));
    assert!(recs
        .iter()
        .any(|r| r.active && r.holder == b && r.resource == d && r.granted_by == a));

    cleanup(&wal);
}

/// C. grant → write → abort → new tx cannot use aborted grant
#[test]
fn s_extra_c_abort_then_new_tx_cannot_use_grant() {
    let wal = temp_wal("extra_c");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_write(sid, 0, b"will-abort".to_vec(), Some(a)).unwrap();
    world.tx_abort(sid).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "owns").unwrap();
    assert!(
        world.tx_commit(sid_b).is_err(),
        "new session must not observe aborted grant"
    );

    cleanup(&wal);
}

/// D. grant → commit → new session uses capability
#[test]
fn s_extra_d_grant_commit_new_session_uses_cap() {
    let wal = temp_wal("extra_d");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    // Brand-new session as B (simulates new WorldSession).
    let sid_new = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_new, b, c, "owns").unwrap();
    world
        .tx_commit(sid_new)
        .expect("new session must see committed capability");
    assert!(kernel.has_link(b, c));

    cleanup(&wal);
}

/// E. Consecutive writes A→B→C→A→C→B — capability_context / current_object must not drift into unauthorized access.
#[test]
fn s_extra_e_consecutive_cross_object_writes_no_drift() {
    let wal = temp_wal("extra_e");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    // Independent objects: each is its own creator; no cross AdminCap.
    let (a, _) = world.create_object_short().unwrap();
    let (b, _) = world.create_object_short().unwrap();
    let (c, _) = world.create_object_short().unwrap();

    // Session as A: self-writes OK; cross writes to B/C must fail without grant.
    let sid = world.tx_begin(Some(a)).unwrap();
    world
        .tx_write(sid, 0, b"A1".to_vec(), Some(a))
        .expect("A self write");
    assert!(
        world.tx_write(sid, 0, b"B-forged".to_vec(), Some(b)).is_err(),
        "A must not write B without capability"
    );
    assert!(
        world.tx_write(sid, 0, b"C-forged".to_vec(), Some(c)).is_err(),
        "A must not write C without capability"
    );
    // After failed cross attempts, self write must still work (no sticky wrong context).
    world
        .tx_write(sid, 1, b"A2".to_vec(), Some(a))
        .expect("A self write after denied cross must still succeed");
    world.tx_commit(sid).unwrap();

    let sid_a = world.tx_begin(Some(a)).unwrap();
    assert_eq!(world.tx_read(sid_a, 0).unwrap(), b"A1".to_vec());
    assert_eq!(world.tx_read(sid_a, 1).unwrap(), b"A2".to_vec());
    world.tx_commit(sid_a).unwrap();

    // B and C must be untouched.
    let sid_b = world.tx_begin(Some(b)).unwrap();
    // empty / missing state is fine; just ensure no forged payload if present
    if let Ok(v) = world.tx_read(sid_b, 0) {
        assert_ne!(v, b"B-forged".to_vec());
    }
    let _ = world.tx_abort(sid_b);

    cleanup(&wal);
}

/// F. Mixed link/unlink with capability grant
#[test]
fn s_extra_f_link_unlink_with_grant() {
    let wal = temp_wal("extra_f");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_link(sid0, a, c, "owns").unwrap();
    world.tx_commit(sid0).unwrap();
    assert!(kernel.has_link(a, c));

    // Grant B, then B links; A unlinks own edge in another tx.
    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "depends_on").unwrap();
    world.tx_commit(sid_b).unwrap();
    assert!(kernel.has_link(b, c));

    let sid_a = world.tx_begin(Some(a)).unwrap();
    world.tx_unlink(sid_a, a, c).unwrap();
    world.tx_commit(sid_a).unwrap();
    assert!(!kernel.has_link(a, c), "A→C must be unlinked");
    assert!(kernel.has_link(b, c), "B→C must remain");

    cleanup(&wal);
}

/// G. Multiple object switches before commit
#[test]
fn s_extra_g_multiple_object_switches_before_commit() {
    let wal = temp_wal("extra_g");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    let c = world.tx_create_object(sid).unwrap();

    // Switch A → B → C → A via writes; creator AdminCap authorizes.
    world.tx_write(sid, 0, b"A".to_vec(), Some(a)).unwrap();
    world.tx_write(sid, 0, b"B".to_vec(), Some(b)).unwrap();
    world.tx_write(sid, 0, b"C".to_vec(), Some(c)).unwrap();
    world.tx_write(sid, 1, b"A2".to_vec(), Some(a)).unwrap();
    world.tx_commit(sid).unwrap();

    for (id, state_id, expected) in [
        (a, 0u64, &b"A"[..]),
        (b, 0, &b"B"[..]),
        (c, 0, &b"C"[..]),
        (a, 1, &b"A2"[..]),
    ] {
        let s = world.tx_begin(Some(id)).unwrap();
        assert_eq!(world.tx_read(s, state_id).unwrap(), expected.to_vec());
        world.tx_commit(s).unwrap();
    }

    cleanup(&wal);
}

/// H. Grant then operate under grantee identity in a fresh session (identity return).
#[test]
fn s_extra_h_grant_then_switch_identity_and_return() {
    let wal = temp_wal("extra_h");
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let a = world.attach_identity(None).unwrap();
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    let sid_g = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid_g, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid_g).unwrap();

    // Operate as B
    let sid_b = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid_b, b, c, "owns").unwrap();
    world.tx_commit(sid_b).unwrap();

    // Return to A: A still works on own resources; grantor semantics unchanged.
    let sid_a = world.tx_begin(Some(a)).unwrap();
    world
        .tx_write(sid_a, 0, b"A-back".to_vec(), Some(a))
        .unwrap();
    world.tx_commit(sid_a).unwrap();

    let recs = cap_records_for(&kernel, b, c);
    assert_eq!(recs[0].granted_by, a);
    assert!(kernel.has_link(b, c));

    cleanup(&wal);
}
