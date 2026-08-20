//! Checkpoint Commitment Domain structural integrity.
//!
//! Beyond referential endpoints (Gap 5), restore must reject snapshots that
//! are internally self-consistent at the byte/commitment layer but violate
//! uniqueness or Capability forest shape required by Constitution.
//!
//! Closable with WorldSnapshot fields only — no Serialization Contract extension.
//!
//! Constitution:
//! - object.md: ObjectId globally unique
//! - link.md §4.1: same-direction same-type Link must not repeat; no self-loop
//! - memory.md: Address = (ObjectId, StateId) is unique key in StateStore
//! - capability graph is a forest: one root per capability_id; parent must be
//!   an existing holder of the same capability_id; (capability_id, holder) unique
//!
//! Residuals (not closed here):
//! - terminal Delta binding for last_applied_delta_hash

use veritas_kernel::engine::state_commitment_from_components;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{
    CapabilitySemanticRecord, LinkSnapshot, LinkType, ObjectSnapshot, ObjectState, ObjectType,
    Address, StateEntry,
};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_structint_{}_{}.wal",
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

/// RED: duplicate ObjectId in ObjectRegistry must be rejected.
/// Even with matching commitment, HashMap last-wins would make post-restore
/// root_hash diverge from claimed state_commitment.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_duplicate_object_id_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("dup_obj_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let dup = snap.objects[0].clone();
    snap.objects.push(dup);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("dup_obj_dst"));
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);

    assert!(
        !k2.test_restore_checkpoint(&snap),
        "duplicate ObjectId must be rejected"
    );
    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate Engine"
    );
}

/// RED: duplicate Address in state_entries must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_duplicate_state_address_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("dup_addr_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(!snap.state_entries.is_empty());
    let dup = snap.state_entries[0].clone();
    snap.state_entries.push(dup);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("dup_addr_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "duplicate Address must be rejected"
    );
}

/// RED: duplicate same-direction same-type Link must be rejected (link.md §4.1).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_duplicate_link_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("dup_link_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let edge = LinkSnapshot {
        from: a,
        to: b,
        link_type: LinkType::References,
    };
    snap.links.push(edge.clone());
    snap.links.push(edge);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("dup_link_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "duplicate (from,to,type) Link must be rejected"
    );
}

/// RED: capability parent that is not a holder of the same capability_id.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_parent_not_holder_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_parent_src"));
    let grantor = birth(&kernel);
    let holder = birth(&kernel);
    let resource = birth(&kernel);
    let stranger = birth(&kernel);
    write_state(&kernel, grantor, 1, b"g".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    // Forge a single-record tree with parent pointing at stranger who is not
    // a holder of this capability_id. Root count becomes 0; parent not in holders.
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0xDEAD_BEEF,
        granted_by: grantor,
        holder,
        resource,
        capability_type: "read".to_string(),
        active: true,
        parent: Some(stranger),
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_parent_dst"));
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);

    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability parent must be a holder of the same capability_id"
    );
    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate Engine"
    );
}

/// RED: capability_id with zero roots (all records have parent) is illegal forest.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_zero_roots_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_0root_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    // Two holders, mutual parents — zero true roots.
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0xCAFE,
        granted_by: a,
        holder: b,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: Some(c),
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0xCAFE,
        granted_by: a,
        holder: c,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: Some(b),
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_0root_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability_id with zero roots must be rejected"
    );
}

/// RED: capability_id with two roots is illegal forest.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_two_roots_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_2root_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0xBEEF,
        granted_by: a,
        holder: b,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: None,
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0xBEEF,
        granted_by: a,
        holder: c,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: None,
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_2root_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability_id with two roots must be rejected"
    );
}

/// RED: same capability_id with inconsistent granted_by / resource / type.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_inconsistent_meta_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_meta_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    let d = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0x1111,
        granted_by: a,
        holder: b,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: None,
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0x1111,
        granted_by: d, // inconsistent granted_by
        holder: c,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: Some(b),
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_meta_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "inconsistent grant metadata for same capability_id must be rejected"
    );
}

/// RED: self-parent on capability record must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_self_parent_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_self_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    // Need a valid root + a child that self-parents (illegal)
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0x2222,
        granted_by: a,
        holder: b,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: None,
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: 0x2222,
        granted_by: a,
        holder: c,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: Some(c), // self-parent
        cascade_on_revoke: true,
                grant_sequence: 1,
            });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_self_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability self-parent must be rejected"
    );
}

/// RED: duplicate (capability_id, holder) must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn red_cap_duplicate_holder_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("cap_dup_h_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let rec = CapabilitySemanticRecord {
        capability_id: 0x3333,
        granted_by: a,
        holder: b,
        resource: c,
        capability_type: "read".to_string(),
        active: true,
        parent: None,
        cascade_on_revoke: true,
                grant_sequence: 1,
            };
    snap.capability_records.push(rec.clone());
    snap.capability_records.push(rec);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("cap_dup_h_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "duplicate (capability_id, holder) must be rejected"
    );
}

/// GREEN: honest checkpoint remains accepted after structural checks.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn green_honest_structural_accepted() {
    let kernel = Kernel::with_wal_path(temp_wal("ok_struct_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());
    write_state(&kernel, b, 1, b"b".to_vec());

    let snap = kernel.test_create_checkpoint();
    let k2 = Kernel::with_wal_path(temp_wal("ok_struct_dst"));
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(k2.test_engine().root_hash(), snap.state_commitment);
}

/// GREEN: legal capability forest (root + child) is accepted.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-16
#[test]
fn green_legal_cap_forest_accepted() {
    // Birth mints creator AdminCap roots with real grant_sequence binding.
    let kernel = Kernel::with_wal_path(temp_wal("ok_cap_src"));
    let a = birth(&kernel);
    let _b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let snap = kernel.test_create_checkpoint();
    assert!(
        !snap.capability_records.is_empty(),
        "precondition: birth leaves AdminCap records"
    );
    assert!(snap.grant_sequence > 0);
    for rec in &snap.capability_records {
        if rec.parent.is_none() {
            assert!(rec.grant_sequence > 0);
            assert!(rec.grant_sequence <= snap.grant_sequence);
            let expected = veritas_kernel::capability::capability_id_of(
                rec.granted_by,
                rec.holder,
                rec.resource,
                rec.grant_sequence,
            );
            assert_eq!(expected, rec.capability_id);
        }
    }

    let k2 = Kernel::with_wal_path(temp_wal("ok_cap_dst"));
    assert!(
        k2.test_restore_checkpoint(&snap),
        "honest capability forest with sequence binding must be accepted"
    );
}

// Silence unused import warnings if any helpers are kept for future cases.
#[allow(dead_code)]
fn _unused_types() {
    let _ = ObjectSnapshot {
        id: 0,
        object_type: ObjectType::StateObject,
        lifecycle_state: ObjectState::Alive,
        metadata: vec![],
        payload: vec![],
    };
    let _ = Address::new(0, 0);
    let _ = StateEntry {
        value: vec![],
        version: 0,
    };
}
