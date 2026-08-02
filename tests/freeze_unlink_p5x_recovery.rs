use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{LinkType, ObjectState};

/// P5.x: Object Freeze 后 commit,重启(WAL recovery)后状态必须仍是 Frozen。
/// 修复前:Freeze 只改内存 registry,从未写 WAL,重启后静默退回 Alive,
/// 写保护失效。
#[test]
fn freeze_survives_recovery() {
    let wal_path = format!("target/test_freeze_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let target: u64 = 0xF2EE;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, target).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_freeze(&mut tx2, target).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert_eq!(engine.get_object_state(target), Some(ObjectState::Frozen));
    }

    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());
    assert_eq!(
        recovered_engine.get_object_state(target),
        Some(ObjectState::Frozen),
        "Frozen state must survive engine restart via WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Object Unlink 后 commit,重启后这条边必须仍然是解除状态,
/// 不能复活。修复前:Unlink 只改内存 topology,从未写 WAL。
#[test]
fn unlink_survives_recovery() {
    let wal_path = format!("target/test_unlink_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64 = 0x10001;
    let b: u64 = 0x10002;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_link(&mut tx2, a, b, LinkType::References).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        engine.object_unlink(&mut tx3, a, b).unwrap();
        engine.commit(&mut tx3).unwrap();
    }

    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());
    assert!(
        !recovered_engine.has_link(a, b),
        "Unlinked edge must not reappear after WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// 对照组:确认 has_link 本身可靠——一条从未 unlink 的边,
/// recovery 后必须依然存在(避免 unlink_survives_recovery 因
/// has_link 恒为 false 而产生假阳性)。
#[test]
fn link_without_unlink_survives_recovery() {
    let wal_path = format!("target/test_link_control_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64 = 0x20001;
    let b: u64 = 0x20002;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_link(&mut tx2, a, b, LinkType::References).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert!(engine.has_link(a, b), "sanity check before restart failed");
    }

    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());
    assert!(
        recovered_engine.has_link(a, b),
        "Link that was never unlinked must still exist after recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}
