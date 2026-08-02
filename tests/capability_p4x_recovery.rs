use veritas_kernel::capability::capability_id_of;
use veritas_kernel::engine::VeritasEngine;

/// P4.x: Object 创建时授予的 AdminCap 必须在 commit 后正确写入
/// capability_graph（此前只在事务内 pending，从未真正问题；
/// 这里验证的是最基本的正向路径）。
#[test]
fn capability_grant_visible_after_commit() {
    let wal_path = format!("target/test_cap_visible_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);
    let engine = VeritasEngine::with_wal_path(wal_path.clone());

    let target: u64 = 0xC0FFEE01;
    let seq_before = engine.capability_sequence();

    let mut tx = engine.begin();
    engine.object_birth(&mut tx, target).unwrap();
    engine.commit(&mut tx).unwrap();

    let expected_cap_id = capability_id_of(target, target, target, seq_before + 1);
    assert!(
        engine.holds_capability(expected_cap_id, target),
        "AdminCap should be held by object after commit"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x 核心修复验证：Object 创建并 commit 后，重启引擎（模拟 crash + restart，
/// 用同一个 WAL 文件重新构造 engine），AdminCap 必须仍然存在。
/// 修复前：Recovery 只重放了 revoke_holder，从未重放 grant，
/// 重启后所有 Capability 会全部丢失。
#[test]
fn capability_survives_recovery() {
    let wal_path = format!("target/test_cap_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let target: u64 = 0xC0FFEE02;
    let expected_cap_id;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let seq_before = engine.capability_sequence();
        expected_cap_id = capability_id_of(target, target, target, seq_before + 1);

        let mut tx = engine.begin();
        engine.object_birth(&mut tx, target).unwrap();
        engine.commit(&mut tx).unwrap();

        assert!(
            engine.holds_capability(expected_cap_id, target),
            "sanity check before restart failed"
        );
        // engine 在这里 drop，模拟进程退出
    }

    // 用同一个 WAL 路径重新构造引擎，模拟重启 recovery
    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());

    assert!(
        recovered_engine.holds_capability(expected_cap_id, target),
        "AdminCap must survive engine restart via WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P4.x: object_birth 所在事务如果 abort，AdminCap 不能残留在
/// capability_graph 中（修复前：cap_graph.grant 在 object_birth 内
/// 立即执行，与事务生命周期无关，abort 后仍然残留）。
#[test]
fn capability_grant_no_leak_on_abort() {
    let wal_path = format!("target/test_cap_no_leak_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);
    let engine = VeritasEngine::with_wal_path(wal_path.clone());

    let target: u64 = 0xC0FFEE03;
    let seq_before = engine.capability_sequence();
    let would_be_cap_id = capability_id_of(target, target, target, seq_before + 1);

    let mut tx = engine.begin();
    engine.object_birth(&mut tx, target).unwrap();
    engine.abort(&mut tx, veritas_kernel::types::AbortReason::WriteConflict);

    assert!(
        !engine.holds_capability(would_be_cap_id, target),
        "AdminCap must not leak into capability_graph after abort"
    );

    let _ = std::fs::remove_file(&wal_path);
}
