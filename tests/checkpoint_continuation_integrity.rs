//! Checkpoint Continuation Integrity — Constitution §3.4 Gap closure.
//!
//! Continuity Version Identity = (global_version, last_applied_delta_hash)
//! is an inseparable World State tuple (commit_version.md §3.4 / §4).
//!
//! restore_checkpoint() must reject checkpoints that violate the closed
//! genesis pairing:
//!   global_version == 0  ↔  last_applied_delta_hash == ZERO_HASH
//!
//! Without the terminal Delta carried in WorldSnapshot, this is the strongest
//! structural invariant the restore path can enforce under the current
//! Serialization Contract. State Commitment remains independent
//! (commitment_boundary.md).

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{ObjectType, ZERO_HASH};

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_cont_int_{}_{}.wal",
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

/// RED: version > 0 but last_applied_delta_hash = ZERO_HASH must be rejected.
/// Constitution §3.4 / §4: non-genesis version cannot pair with ZERO_HASH.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn red_nonzero_version_with_zero_hash_rejects() {
    let wal = temp_wal("red_nz_ver_zh");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"payload".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(snap.global_version > 0, "precondition: version advanced");
    assert_ne!(
        snap.last_applied_delta_hash, ZERO_HASH,
        "precondition: hash advanced"
    );

    // Violate §3.4: keep version N, force hash to ZERO_HASH
    snap.last_applied_delta_hash = ZERO_HASH;

    let wal2 = temp_wal("red_nz_ver_zh_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "version>0 + ZERO_HASH must be rejected by restore_checkpoint (§3.4)"
    );
}

/// RED: version == 0 but last_applied_delta_hash != ZERO_HASH must be rejected.
/// Constitution §4: genesis is closed as (0, ZERO_HASH).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn red_zero_version_with_nonzero_hash_rejects() {
    let wal = temp_wal("red_z_ver_nh");
    let kernel = Kernel::with_wal_path(wal);
    // Genesis snapshot
    let mut snap = kernel.test_create_checkpoint();
    assert_eq!(snap.global_version, 0);
    assert_eq!(snap.last_applied_delta_hash, ZERO_HASH);

    // Violate §4: keep version 0, force a non-zero hash
    snap.last_applied_delta_hash = [0xAB; 32];

    let wal2 = temp_wal("red_z_ver_nh_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "version=0 + non-ZERO_HASH must be rejected by restore_checkpoint (§4)"
    );
}

/// Reject path must not pollute the target Engine (same atomicity rule as
/// state_commitment verification).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn red_inconsistent_continuation_rejects_without_pollution() {
    let wal = temp_wal("red_poll_src");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"src".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.last_applied_delta_hash = ZERO_HASH; // inconsistent with version > 0

    let wal2 = temp_wal("red_poll_dst");
    let k2 = Kernel::with_wal_path(wal2);
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);
    let ver_before = k2.get_global_version();
    let hash_before = k2.get_last_applied_delta_hash();

    assert!(!k2.test_restore_checkpoint(&snap));

    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate target state"
    );
    assert_eq!(
        k2.get_global_version(),
        ver_before,
        "failed restore must not mutate global_version"
    );
    assert_eq!(
        k2.get_last_applied_delta_hash(),
        hash_before,
        "failed restore must not mutate last_applied_delta_hash"
    );
}

/// GREEN: honest checkpoint roundtrip preserves Continuity Version Identity.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn green_valid_continuation_roundtrip() {
    let wal = temp_wal("green_rt_src");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"ok".to_vec());

    let snap = kernel.test_create_checkpoint();
    assert!(snap.global_version > 0);
    assert_ne!(snap.last_applied_delta_hash, ZERO_HASH);

    let wal2 = temp_wal("green_rt_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(k2.get_global_version(), snap.global_version);
    assert_eq!(
        k2.get_last_applied_delta_hash(),
        snap.last_applied_delta_hash
    );
    assert_eq!(k2.test_engine().root_hash(), snap.state_commitment);
}

/// GREEN: genesis checkpoint (0, ZERO_HASH) is accepted.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn green_genesis_zero_hash_accepted() {
    let wal = temp_wal("green_gen_src");
    let kernel = Kernel::with_wal_path(wal);
    let snap = kernel.test_create_checkpoint();
    assert_eq!(snap.global_version, 0);
    assert_eq!(snap.last_applied_delta_hash, ZERO_HASH);

    let wal2 = temp_wal("green_gen_dst");
    let k2 = Kernel::with_wal_path(wal2);
    // Put some state on target so we can observe restore
    let _ = birth(&k2);
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(k2.get_global_version(), 0);
    assert_eq!(k2.get_last_applied_delta_hash(), ZERO_HASH);
}

/// GREEN: tampering continuation hash to a *different non-zero* value while
/// keeping version is currently accepted at the structural layer — the
/// checkpoint does not carry the terminal Delta, so restore cannot recompute
/// content_hash(terminal_delta(N)). Documented residual of the Serialization
/// Contract; State Commitment still independently protects the five components.
///
/// This test locks the residual so a future architecture change that closes it
/// will fail loudly here and force an intentional update.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-13
#[test]
fn residual_nonzero_hash_swap_still_accepted_without_terminal_delta() {
    let wal = temp_wal("resid_src");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"x".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(snap.global_version > 0);
    let original = snap.last_applied_delta_hash;
    // Swap to another non-zero hash (not ZERO_HASH, not original)
    let mut forged = [0x11u8; 32];
    if forged == original {
        forged[0] = 0x22;
    }
    snap.last_applied_delta_hash = forged;

    let wal2 = temp_wal("resid_dst");
    let k2 = Kernel::with_wal_path(wal2);
    // Structural check only: version>0 + non-ZERO is accepted.
    // Full cryptographic binding of (N, H_terminal) requires terminal Delta
    // in the checkpoint — out of scope for this gap closure.
    assert!(
        k2.test_restore_checkpoint(&snap),
        "without terminal Delta in snapshot, non-ZERO forged hash is structurally accepted"
    );
    assert_eq!(k2.get_last_applied_delta_hash(), forged);
}
