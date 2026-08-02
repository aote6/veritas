#!/usr/bin/env python3
import shutil, datetime

def backup(path):
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak_{ts}"
    shutil.copy(path, bak)
    print(f"[backup] {path} -> {bak}")

def apply(path, edits):
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    backup(path)
    for desc, old, new in edits:
        count = content.count(old)
        if count != 1:
            print(f"[FAIL] {path}: '{desc}' 锚点出现 {count} 次,跳过整个文件")
            return False
        content = content.replace(old, new, 1)
        print(f"[OK] {path}: {desc}")
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    return True

# 加两个只读查询方法,方便测试断言,不影响任何现有逻辑
engine_edits = [
    ("加 holds_capability 和 capability_sequence 只读方法",
     '''    fn verify_capability(&self, ctx: &crate::types::TransactionContext) -> Result<(), crate::types::VeritasError> {''',
     '''    /// 只读查询：某个 holder 是否持有某个 cap_id 且该 cap 有效。测试与外部诊断用。
    pub fn holds_capability(&self, cap_id: crate::types::CapabilityId, holder: crate::types::ObjectId) -> bool {
        let cap_graph = self.capability_graph.lock().unwrap();
        cap_graph.is_capability_valid(cap_id) && cap_graph.holds(cap_id, holder)
    }

    /// 只读查询：capability_graph 当前的 grant_sequence 计数器值。测试用于推算 cap_id。
    pub fn capability_sequence(&self) -> u64 {
        let cap_graph = self.capability_graph.lock().unwrap();
        cap_graph.current_sequence()
    }

    fn verify_capability(&self, ctx: &crate::types::TransactionContext) -> Result<(), crate::types::VeritasError> {'''),
]

ok = apply("src/engine.rs", engine_edits)

test_file_content = '''use veritas_kernel::capability::capability_id_of;
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
'''

with open("tests/capability_p4x_recovery.rs", "w", encoding="utf-8") as f:
    f.write(test_file_content)
print("[OK] 已创建 tests/capability_p4x_recovery.rs")

if ok:
    print("\n接下来跑: cargo test capability_p4x 2>&1 | tail -60")
else:
    print("\nengine.rs 锚点未匹配,请贴出 [FAIL] 信息")
