//! Recovery / Replay Continuity — 三路等价性。
//!
//! 验证：live 状态的 root_hash 必须与 WAL replay 和 checkpoint
//! restore 得到的 root_hash 完全一致。这是 P30.4 / P30.5 的
//! 正式闭合测试。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn unique_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_rc_{}_{}_{}.wal", name, std::process::id(), rand_seq()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

fn rand_seq() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
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
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit);
    id
}

fn write(kernel: &Kernel, obj: u64, sid: u64, val: Vec<u8>) {
    let mut tx = kernel.test_begin_in_object(obj);
    kernel.test_write(&mut tx, sid, val).unwrap();
    kernel.test_commit(&mut tx).unwrap();
}

/// Live == Replay == Checkpoint Restore
///
/// @category: A
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn live_replay_checkpoint_three_way_equivalence() {
    let wal = unique_wal("three");

    // 1. Live 状态
    let (live_root, snap) = {
        let kernel = Kernel::with_wal_path(wal.clone());
        let a = birth(&kernel);
        let b = birth(&kernel);
        write(&kernel, a, 1, b"alpha".to_vec());
        write(&kernel, b, 2, b"beta".to_vec());
        let root = kernel.test_engine().root_hash();
        let snap = kernel.test_create_checkpoint();
        (root, snap)
    };

    // 2. Checkpoint restore 状态
    let restored_root = {
        let wal2 = unique_wal("restore");
        let k2 = Kernel::with_wal_path(wal2);
        assert!(k2.test_restore_checkpoint(&snap), "valid checkpoint must restore");
        k2.test_engine().root_hash()
    };

    // 3. WAL replay 状态
    let replay_root = Kernel::replay(&wal);

    assert_eq!(live_root, replay_root, "live root must equal WAL replay root");
    assert_eq!(live_root, restored_root, "live root must equal checkpoint restore root");
    assert_eq!(replay_root, restored_root, "replay root must equal checkpoint restore root");
}
