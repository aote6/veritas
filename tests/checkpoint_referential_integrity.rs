//! Checkpoint Commitment Domain referential integrity.
//!
//! State Commitment authenticates the five-component bytes. It does not by
//! itself guarantee that Topology / StateStore / Capability endpoints name
//! ObjectIds present in ObjectRegistry.
//!
//! Constitution:
//! - link.md §4.1: OBJECT_LINK requires from/to exist; forbids self-loop
//! - memory.md: address = (ObjectId, StateId) within the object world
//! - object death cleans StateStore (no state for unknown objects)
//!
//! restore_checkpoint() must reject snapshots that break these references,
//! even when state_commitment has been recomputed to match the tampered bytes.
//! No new Serialization Contract fields.

use veritas_kernel::engine::state_commitment_from_components;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{LinkSnapshot, LinkType, ObjectType};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_refint_{}_{}.wal",
        name,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.test_begin();
    let id = match kernel.handle(
        &mut tx,
        KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        },
    ) {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit);
    id
}

fn write_state(kernel: &Kernel, obj: u64, state_id: u64, payload: Vec<u8>) {
    let mut tx = kernel.test_begin_in_object(obj);
    kernel.test_write(&mut tx, state_id, payload).unwrap();
    kernel.test_commit(&mut tx).unwrap();
}

fn read_state(kernel: &Kernel, obj: u64, state_id: u64) -> Vec<u8> {
    let mut tx = kernel.test_begin_in_object(obj);
    kernel.test_read(&mut tx, state_id).unwrap()
}

fn recompute_commitment(snap: &mut veritas_kernel::types::WorldSnapshot) {
    let objects: Vec<(u64, u8, u8)> = snap
        .objects
        .iter()
        .map(|o| (o.id, o.lifecycle_state as u8, o.object_type as u8))
        .collect();
    let links: Vec<(u64, u64, u8)> = snap
        .links
        .iter()
        .map(|l| (l.from, l.to, l.link_type as u8))
        .collect();
    let caps: Vec<(u64, u64, u64, String)> = snap
        .capability_records
        .iter()
        .filter(|r| r.parent.is_none())
        .map(|r| {
            (
                r.granted_by,
                r.holder,
                r.resource,
                r.capability_type.clone(),
            )
        })
        .collect();
    let scopes: Vec<(u64, Vec<u64>, u64)> = snap
        .scopes
        .iter()
        .map(|s| (s.scope_id, s.members.clone(), s.struct_version))
        .collect();
    snap.state_commitment =
        state_commitment_from_components(&snap.state_entries, &objects, &links, &caps, &scopes);
}

/// RED: dangling link endpoint must be rejected even if commitment matches.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn red_dangling_link_rejects_despite_matching_commitment() {
    let kernel = Kernel::with_wal_path(temp_wal("dl_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.links.push(LinkSnapshot {
        from: a,
        to: 999_999,
        link_type: LinkType::References,
    });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("dl_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "dangling link endpoint must be rejected (link.md §4.1)"
    );
}

/// RED: self-loop link must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn red_self_loop_link_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("sl_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.links.push(LinkSnapshot {
        from: a,
        to: a,
        link_type: LinkType::References,
    });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("sl_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "self-loop link must be rejected (link.md §4.1)"
    );
}

/// RED: state_entry for unknown ObjectId must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn red_orphan_state_entry_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("ose_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"payload".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(!snap.state_entries.is_empty());
    snap.state_entries[0].0.object_id = 888_888;
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("ose_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "state_entry for unknown ObjectId must be rejected"
    );
}

/// RED: capability record naming unknown ObjectId must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn red_capability_unknown_endpoint_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_src"));
    let a = birth(&kernel);
    // produce at least one capability via self AdminCap path if available
    let mut tx = kernel.test_begin();
    let _ = kernel.handle(
        &mut tx,
        KernelCall::CapabilityGrant {
            grantor: a,
            grantee: a,
            resource: a,
            capability_type: "AdminCap".into(),
        },
    );
    kernel.handle(&mut tx, KernelCall::Commit);

    let mut snap = kernel.test_create_checkpoint();
    if snap.capability_records.is_empty() {
        // Still exercise path: inject a fabricated record with unknown holder
        snap.capability_records.push(
            veritas_kernel::types::CapabilitySemanticRecord {
                capability_id: 1,
                granted_by: a,
                holder: 777_777,
                resource: a,
                capability_type: "AdminCap".into(),
                active: true,
                parent: None,
                cascade_on_revoke: true,
                grant_sequence: 1,
            },
        );
    } else {
        snap.capability_records[0].holder = 777_777;
    }
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability endpoint naming unknown ObjectId must be rejected"
    );
}

/// Reject must not pollute target Engine.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn red_referential_reject_without_pollution() {
    let kernel = Kernel::with_wal_path(temp_wal("poll_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"src".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.links.push(LinkSnapshot {
        from: a,
        to: 999_999,
        link_type: LinkType::References,
    });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("poll_dst"));
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);

    assert!(!k2.test_restore_checkpoint(&snap));
    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate target state"
    );
}

/// GREEN: honest checkpoint with consistent references is accepted.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-15
#[test]
fn green_honest_references_accepted() {
    let kernel = Kernel::with_wal_path(temp_wal("ok_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());
    write_state(&kernel, b, 1, b"b".to_vec());

    let snap = kernel.test_create_checkpoint();
    let k2 = Kernel::with_wal_path(temp_wal("ok_dst"));
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(k2.test_engine().root_hash(), snap.state_commitment);
}
