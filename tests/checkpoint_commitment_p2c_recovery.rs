//! Phase 2C: Checkpoint Commitment Verification.
//! restore_checkpoint() 必须先验证 snap.state_commitment 与重新计算的
//! commitment 一致，才能恢复；不一致时不得触碰目标 Engine 任何状态。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_p2c_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

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

fn write_state(kernel: &Kernel, obj: u64, state_id: u64, payload: Vec<u8>) {
    let mut tx = kernel.test_begin_in_object(obj);
    kernel.test_write(&mut tx, state_id, payload).unwrap();
    kernel.test_commit(&mut tx).unwrap();
}

fn read_state(kernel: &Kernel, obj: u64, state_id: u64) -> Vec<u8> {
    let mut tx = kernel.test_begin_in_object(obj);
    kernel.test_read(&mut tx, state_id).unwrap()
}

/// RED 1: 篡改 snap.state_commitment，restore 必须返回 false。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-12
#[test]
fn red1_tampered_state_commitment_rejects_restore() {
    let wal = temp_wal("red1");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"hello".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    snap.state_commitment[0] ^= 0xFF;

    let wal2 = temp_wal("red1b");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(
        !k2.test_restore_checkpoint(&snap),
        "篡改 state_commitment 后 restore_checkpoint 必须返回 false"
    );
}

/// RED 2: 篡改 state_entries 里某个 value（保持原 state_commitment 不变），
/// restore 必须返回 false，且目标 Engine 原有状态不得被污染。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-12
#[test]
fn red2_tampered_entry_value_rejects_restore_without_pollution() {
    let wal = temp_wal("red2_src");
    let kernel = Kernel::with_wal_path(wal);
    let obj = birth(&kernel);
    write_state(&kernel, obj, 1, b"hello".to_vec());

    let mut snap = kernel.test_create_checkpoint();
    assert!(!snap.state_entries.is_empty());
    snap.state_entries[0].1.value.push(0xAA);

    let wal2 = temp_wal("red2_dst");
    let k2 = Kernel::with_wal_path(wal2);
    let sentinel_obj = birth(&k2);
    write_state(&k2, sentinel_obj, 1, b"SENTINEL".to_vec());
    let before = read_state(&k2, sentinel_obj, 1);

    assert!(
        !k2.test_restore_checkpoint(&snap),
        "篡改 state_entries 后 restore_checkpoint 必须返回 false"
    );

    let after = read_state(&k2, sentinel_obj, 1);
    assert_eq!(before, after, "restore 失败后目标 Engine 原有状态不得被污染");
}

/// GREEN: 合法 checkpoint restore 必须返回 true，
/// 且恢复后 root_hash() == snap.state_commitment。
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-12
#[test]
fn green_valid_checkpoint_restore_matches_root_hash() {
    let wal = temp_wal("green_src");
    let kernel = Kernel::with_wal_path(wal);
    let a = birth(&kernel);
    let b = birth(&kernel);
    write_state(&kernel, a, 1, b"alpha".to_vec());
    write_state(&kernel, b, 1, b"beta".to_vec());

    let snap = kernel.test_create_checkpoint();

    let wal2 = temp_wal("green_dst");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(k2.test_restore_checkpoint(&snap), "合法 checkpoint restore 必须返回 true");
    assert_eq!(
        k2.test_engine().root_hash(),
        snap.state_commitment,
        "restore 后 root_hash() 必须与 snap.state_commitment 一致"
    );
}
