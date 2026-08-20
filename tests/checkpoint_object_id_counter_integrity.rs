//! Checkpoint ObjectId Counter Integrity — Constitution object.md / world.md.
//!
//! ObjectId 不可重用. object_id_counter 决定下一次 ObjectId 分配.
//! next_object_id() returns the current counter value then increments.
//!
//! Therefore restore_checkpoint() must reject any WorldSnapshot where
//! some object.id >= object_id_counter. Otherwise the next birth after
//! restore can collide with an existing ObjectId.
//!
//! This is a structural Continuation Metadata check using data already
//! present in WorldSnapshot — no new Serialization Contract fields.

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "veritas_oid_ctr_{}_{}.wal",
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

/// RED: object_id_counter == max(object.id) must be rejected.
/// Next birth would reuse that id (fetch_add returns current value).
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-14
#[test]
fn red_object_id_counter_equal_max_id_rejects() {
    let wal = temp_wal("red_eq");
    let kernel = Kernel::with_wal_path(wal);
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());
    write_state(&kernel, b, 1, b"b".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let max_id = snap.objects.iter().map(|o| o.id).max().unwrap();
    assert!(
        snap.object_id_counter > max_id,
        "precondition: honest counter strictly above max id"
    );

    snap.object_id_counter = max_id;

    let wal2 = temp_wal("red_eq_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "object_id_counter == max(object.id) must be rejected (ObjectId 不可重用)"
    );
}

/// RED: object_id_counter < max(object.id) must be rejected.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-14
#[test]
fn red_object_id_counter_below_max_id_rejects() {
    let wal = temp_wal("red_lt");
    let kernel = Kernel::with_wal_path(wal);
    let _a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, b, 1, b"b".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let max_id = snap.objects.iter().map(|o| o.id).max().unwrap();
    assert!(max_id >= 1);
    snap.object_id_counter = max_id.saturating_sub(1).max(0);

    // Ensure we actually created an inconsistency
    assert!(
        snap.objects.iter().any(|o| o.id >= snap.object_id_counter),
        "precondition: at least one id >= counter"
    );

    let wal2 = temp_wal("red_lt_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "object_id_counter < max(object.id) must be rejected"
    );
}

/// Reject must not pollute target Engine.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-14
#[test]
fn red_low_counter_rejects_without_pollution() {
    let wal = temp_wal("red_poll_src");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"src".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    let max_id = snap.objects.iter().map(|o| o.id).max().unwrap();
    snap.object_id_counter = max_id;

    let wal2 = temp_wal("red_poll_dst");
    let k2 = Kernel::with_wal_path(wal2);
    let sentinel = birth(&k2);
    write_state(&k2, sentinel, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel, 1);
    let ctr_before = k2.test_create_checkpoint().object_id_counter;

    assert!(!k2.test_restore_checkpoint(&snap));

    assert_eq!(
        read_state(&k2, sentinel, 1),
        before,
        "failed restore must not mutate target state"
    );
    assert_eq!(
        k2.test_create_checkpoint().object_id_counter,
        ctr_before,
        "failed restore must not mutate object_id_counter"
    );
}

/// GREEN: honest checkpoint with counter > max id is accepted.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-14
#[test]
fn green_honest_counter_above_max_accepted() {
    let wal = temp_wal("green_ok");
    let kernel = Kernel::with_wal_path(wal);
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"a".to_vec());
    write_state(&kernel, b, 1, b"b".to_vec());

    let snap = kernel.test_create_checkpoint();
    let max_id = snap.objects.iter().map(|o| o.id).max().unwrap();
    assert!(snap.object_id_counter > max_id);

    let wal2 = temp_wal("green_ok_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(k2.test_restore_checkpoint(&snap));
    assert_eq!(
        k2.test_create_checkpoint().object_id_counter,
        snap.object_id_counter
    );

    // Next birth must not collide with any restored id
    let next = birth(&k2);
    assert!(
        !snap.objects.iter().any(|o| o.id == next),
        "post-restore birth must not reuse a restored ObjectId"
    );
    assert_eq!(next, snap.object_id_counter);
}

/// GREEN: empty-object genesis (counter == 1, no objects) is accepted.
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-14
#[test]
fn green_genesis_empty_objects_accepted() {
    let wal = temp_wal("green_gen");
    let kernel = Kernel::with_wal_path(wal);
    let snap = kernel.test_create_checkpoint();
    assert!(snap.objects.is_empty());
    assert!(snap.object_id_counter >= 1);

    let wal2 = temp_wal("green_gen_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(k2.test_restore_checkpoint(&snap));
}
