//! Checkpoint grant_sequence ↔ CapabilitySemanticRecord binding.
//!
//! Serialization Contract extension: CapabilitySemanticRecord.grant_sequence
//! persists the sequence that minted capability_id (canonical input to
//! capability_id_of). restore_checkpoint rejects:
//! - root grant_sequence == 0
//! - root grant_sequence > snap.grant_sequence (counter under-bound)
//! - capability_id != capability_id_of(granted_by, holder, resource, seq)
//! - inconsistent grant_sequence across holders of the same capability_id
//!
//! Does not put sequence into State Commitment encoding.
//! Does not introduce a second allocator — CapabilityGraph.grant_sequence
//! remains the sole monotonic counter.
//!
//! Residual still open: terminal Delta body binding for Continuity Version
//! Identity beyond genesis pairing (requires terminal Delta in snapshot or
//! external receipt/WAL verification).

use veritas_kernel::capability::capability_id_of;
use veritas_kernel::engine::state_commitment_from_components;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{CapabilitySemanticRecord, ObjectType};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_grantseq_{}_{}.wal",
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

/// RED: grant_sequence counter below a root record's sequence must reject.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-17
#[test]
fn red_grant_sequence_counter_below_root_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("gs_below_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(!snap.capability_records.is_empty());
    assert!(snap.grant_sequence > 0);

    // Force counter strictly below every root sequence.
    let min_root_seq = snap
        .capability_records
        .iter()
        .filter(|r| r.parent.is_none())
        .map(|r| r.grant_sequence)
        .min()
        .unwrap();
    snap.grant_sequence = min_root_seq.saturating_sub(1);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("gs_below_dst"));
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);

    assert!(
        !k2.test_restore_checkpoint(&snap),
        "grant_sequence counter below root sequences must be rejected"
    );
    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate Engine"
    );
}

/// RED: capability_id not matching capability_id_of(..., grant_sequence).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-17
#[test]
fn red_capability_id_mismatch_sequence_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("gs_mismatch_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let root = snap
        .capability_records
        .iter_mut()
        .find(|r| r.parent.is_none())
        .expect("AdminCap root");
    // Tamper id while keeping sequence — breaks hash binding.
    root.capability_id = root.capability_id.wrapping_add(1);
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("gs_mismatch_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "capability_id must equal capability_id_of(..., grant_sequence)"
    );
}

/// RED: root grant_sequence == 0 is illegal (sequence starts at 1 on grant).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-17
#[test]
fn red_zero_root_sequence_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("gs_zero_src"));
    let a = birth(&kernel);
    write_state(&kernel, a, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let root_idx = snap
        .capability_records
        .iter()
        .position(|r| r.parent.is_none())
        .expect("AdminCap root");
    let gb = snap.capability_records[root_idx].granted_by;
    let holder = snap.capability_records[root_idx].holder;
    let resource = snap.capability_records[root_idx].resource;
    let old_id = snap.capability_records[root_idx].capability_id;
    let new_id = capability_id_of(gb, holder, resource, 0);
    for rec in snap.capability_records.iter_mut() {
        if rec.capability_id == old_id {
            rec.capability_id = new_id;
            rec.grant_sequence = 0;
        }
    }
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("gs_zero_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "root grant_sequence == 0 must be rejected"
    );
}

/// RED: inconsistent grant_sequence across holders of same capability_id.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-17
#[test]
fn red_inconsistent_sequence_meta_rejects() {
    let kernel = Kernel::with_wal_path(temp_wal("gs_meta_src"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    // Find an AdminCap root and forge a child with different grant_sequence.
    let root = snap
        .capability_records
        .iter()
        .find(|r| r.parent.is_none())
        .cloned()
        .expect("root");
    snap.capability_records.push(CapabilitySemanticRecord {
        capability_id: root.capability_id,
        granted_by: root.granted_by,
        holder: b,
        resource: root.resource,
        capability_type: root.capability_type.clone(),
        active: true,
        parent: Some(root.holder),
        cascade_on_revoke: true,
        grant_sequence: root.grant_sequence.wrapping_add(99),
    });
    recompute_commitment(&mut snap);

    let k2 = Kernel::with_wal_path(temp_wal("gs_meta_dst"));
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "inconsistent grant_sequence on same capability_id must be rejected"
    );
}

/// GREEN: honest checkpoint after birth preserves sequence binding and restores.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-17
#[test]
fn green_honest_grant_sequence_binding() {
    let kernel = Kernel::with_wal_path(temp_wal("gs_ok_src"));
    let a = birth(&kernel);
    let _b = birth(&kernel);
    write_state(&kernel, a, 1, b"payload".to_vec());

    let snap = kernel.test_create_checkpoint();
    assert!(snap.grant_sequence > 0);
    for rec in &snap.capability_records {
        if rec.parent.is_none() {
            assert_eq!(
                capability_id_of(rec.granted_by, rec.holder, rec.resource, rec.grant_sequence),
                rec.capability_id
            );
            assert!(rec.grant_sequence <= snap.grant_sequence);
            assert!(rec.grant_sequence > 0);
        }
    }

    let k2 = Kernel::with_wal_path(temp_wal("gs_ok_dst"));
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(k2.test_engine().root_hash(), snap.state_commitment);
    let restored = k2.test_create_checkpoint();
    assert_eq!(restored.grant_sequence, snap.grant_sequence);
}
