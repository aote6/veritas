//! Commitment Boundary regression tests.
//!
//! These tests lock the ADR commitment_boundary.md decision:
//! State Identity (five components) is decoupled from
//! Continuation Identity (global_version, object_id_counter,
//! grant_sequence, last_applied_delta_hash).
//!
//! If any of these tests fail, the implementation has drifted
//! from the accepted architecture boundary.

use std::sync::Arc;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::kernel::KernelCall;
use veritas_kernel::kernel::TrapResult;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{Address, LinkType, ObjectType, TransactionDelta, ZERO_HASH};
use veritas_kernel::world_api::WorldService;

fn temp_wal(name: &str) -> String {
    format!("target/test_cb_{}_{}.wal", name, std::process::id())
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
}

fn world_pair(name: &str) -> (String, Arc<Kernel>, WorldService) {
    let wal = temp_wal(name);
    let kernel = Arc::new(Kernel::with_wal_path(wal.clone()));
    let world = WorldService::with_wal(Arc::clone(&kernel), wal.clone());
    (wal, kernel, world)
}

fn birth_kernel(kernel: &Kernel) -> u64 {
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
    kernel.handle(&mut tx, KernelCall::Commit);
    id
}

fn make_delta(
    tx_id: u64,
    version: u64,
    actor_id: u64,
    writes: Vec<(Address, Vec<u8>)>,
    births: Vec<u64>,
    deaths: Vec<u64>,
    freezes: Vec<u64>,
    links: Vec<(u64, u64, LinkType)>,
    unlinks: Vec<(u64, u64)>,
    effects: Vec<(String, Vec<u8>)>,
) -> TransactionDelta {
    TransactionDelta {
        tx_id,
        commit_version: version,
        actor_id,
        writes,
        scope_changes: vec![],
        births,
        deaths,
        freezes,
        links,
        unlinks,
        capability_grants: vec![],
        capability_delegates: vec![],
        capability_revokes: vec![],
        effects,
    }
}

// ========== TEST 1: State Commitment excludes Continuation Metadata ==========

/// Two worlds with identical five components but different
/// global_version MUST have the same state commitment.
///
/// ADR commitment_boundary.md §2.3:
/// Continuation Metadata does not enter State Commitment Domain.
///
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-11
#[test]
fn state_commitment_excludes_global_version() {
    let (wal, kernel, _world) = world_pair("sce_gv");

    // World A: one object, one state entry, no deltas applied.
    birth_kernel(&kernel);
    let root_a_before = kernel.state_root();

    // Apply a delta that changes global_version but does not touch
    // any of the five components.
    let d = make_delta(
        1,      // tx_id
        1,      // version: global_version 0 -> 1
        0,      // actor_id
        vec![], // writes: none
        vec![], // births: none
        vec![], // deaths: none
        vec![], // freezes: none
        vec![], // links: none
        vec![], // unlinks: none
        vec![], // effects: none
    );
    kernel.test_apply(&d);

    let root_a_after = kernel.state_root();
    assert_eq!(
        root_a_before, root_a_after,
        "state commitment must not change when only global_version changes"
    );

    // Sanity: global_version did change.
    assert_eq!(kernel.get_global_version(), 1);

    cleanup(&wal);
}

// ========== TEST 2: Checkpoint preserves Continuation Identity ==========

/// A checkpoint must preserve last_applied_delta_hash exactly.
///
/// ADR commitment_boundary.md §2.3:
/// Continuation Identity must survive checkpoint and restore.
///
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-11
#[test]
fn checkpoint_preserves_last_applied_delta_hash() {
    let (wal, kernel, _world) = world_pair("cp_ldh");

    // Apply a delta so last_applied_delta_hash is non-zero.
    let d = make_delta(
        1,
        1,
        0,
        vec![],
        vec![1], // births: one object
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d);
    let h_before = kernel.get_last_applied_delta_hash();
    assert_ne!(h_before, ZERO_HASH);

    // Create checkpoint, restore into a fresh kernel, compare.
    let snap = kernel.test_create_checkpoint();
    let (wal2, kernel2, _world2) = world_pair("cp_ldh_restore");
    assert!(kernel2.test_restore_checkpoint(&snap));

    let h_after = kernel2.get_last_applied_delta_hash();
    assert_eq!(
        h_before, h_after,
        "checkpoint must preserve last_applied_delta_hash"
    );

    cleanup(&wal);
    cleanup(&wal2);
}

// ========== TEST 3: Delta Identity is independent of State Commitment ==========

/// Two worlds with identical five components but different
/// last_applied_delta_hash MUST have the same state commitment.
///
/// ADR commitment_boundary.md §2.2 / §2.3:
/// Delta Identity and State Commitment are separate lines.
///
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-11
#[test]
fn delta_identity_independent_of_state_commitment() {
    let (wal, kernel, _world) = world_pair("di_isc");

    // Baseline root with no deltas.
    let root_baseline = kernel.state_root();

    // Apply delta A: births object 1.
    let d_a = make_delta(
        1,
        1,
        0,
        vec![],
        vec![1],
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
    );
    kernel.test_apply(&d_a);
    let root_after_a = kernel.state_root();
    let hash_after_a = kernel.get_last_applied_delta_hash();
    assert_ne!(root_baseline, root_after_a);
    assert_ne!(hash_after_a, ZERO_HASH);

    // Create a second kernel, restore the same five-component state,
    // but with a different last_applied_delta_hash.
    let (wal2, kernel2, _world2) = world_pair("di_isc_2");
    let snap = kernel.test_create_checkpoint();
    assert!(kernel2.test_restore_checkpoint(&snap));

    // Manually overwrite the restored last_applied_delta_hash
    // to simulate a different Delta Identity with the same five
    // components. This is a direct field manipulation to test
    // the boundary, not a production path.
    let mut snap2 = kernel2.test_create_checkpoint();
    snap2.last_applied_delta_hash = [7u8; 32];
    assert!(kernel2.test_restore_checkpoint(&snap2));

    let root_after_restore = kernel2.state_root();
    let hash_after_restore = kernel2.get_last_applied_delta_hash();

    assert_eq!(
        root_after_a, root_after_restore,
        "same five components must produce same state commitment"
    );
    assert_ne!(
        hash_after_a, hash_after_restore,
        "different last_applied_delta_hash must be preserved independently"
    );

    cleanup(&wal);
    cleanup(&wal2);
}
