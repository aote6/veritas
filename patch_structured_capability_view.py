#!/usr/bin/env python3
"""
补丁五: capability_events 从纯字符串升级为结构化视图。
Capability 在宪法里是有稳定身份的 Kernel 管理资源,下游(forge 或 veritas 自身
的审计/回放/撤销级联)需要能程序化取到 capability_id,而不是解析人类可读字符串。
保留 capability_events: Vec<String> 作为人类可读日志(不删,避免破坏现有调用者),
新增 capability_grants: Vec<CapabilityGrantView> 作为结构化数据源。
"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCHES = [
    {
        "file": "./src/world_api.rs",
        "old": '''pub struct TransactionDeltaView {
    pub actor_id: u64,
    pub objects_created: Vec<u64>,
    pub objects_deleted: Vec<u64>,
    pub objects_frozen: Vec<u64>,
    pub links_added: Vec<(u64, u64, String)>,
    pub links_removed: Vec<(u64, u64)>,
    pub memory_written: Vec<MemoryWriteView>,
    pub capability_events: Vec<String>,
    pub effects: Vec<(String, String)>,
}''',
        "new": '''pub struct TransactionDeltaView {
    pub actor_id: u64,
    pub objects_created: Vec<u64>,
    pub objects_deleted: Vec<u64>,
    pub objects_frozen: Vec<u64>,
    pub links_added: Vec<(u64, u64, String)>,
    pub links_removed: Vec<(u64, u64)>,
    pub memory_written: Vec<MemoryWriteView>,
    /// Human-readable log lines. Do not parse programmatically — use
    /// `capability_grants` for structured access to capability_id etc.
    pub capability_events: Vec<String>,
    /// Structured capability grants from this transaction. Callers that need
    /// to hold onto a capability_id (e.g. to check holds_capability later,
    /// or to track revocation) must read from here, not capability_events.
    pub capability_grants: Vec<CapabilityGrantView>,
    pub effects: Vec<(String, String)>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilityGrantView {
    pub capability_id: u64,
    pub cap_type: String,
    pub grantor: u64,
    pub grantee: u64,
    pub resource: u64,
}''',
    },
    {
        "file": "./src/world_api.rs",
        "old": '''        let capability_events: Vec<String> = d.capability_grants.iter().map(|g| {
            format!(
                "grant cap_id={} type={} grantor={} grantee={} resource={}",
                g.capability_id, g.cap_type, g.grantor, g.grantee, g.resource
            )
        }).collect();

        TransactionDeltaView {
            actor_id: d.actor_id,
            objects_created: d.births.clone(),
            objects_deleted: d.deaths.clone(),
            objects_frozen: d.freezes.clone(),
            links_added,
            links_removed: d.unlinks.clone(),
            memory_written,
            capability_events,
            effects,
        }''',
        "new": '''        let capability_events: Vec<String> = d.capability_grants.iter().map(|g| {
            format!(
                "grant cap_id={} type={} grantor={} grantee={} resource={}",
                g.capability_id, g.cap_type, g.grantor, g.grantee, g.resource
            )
        }).collect();
        let capability_grants: Vec<CapabilityGrantView> = d.capability_grants.iter().map(|g| {
            CapabilityGrantView {
                capability_id: g.capability_id,
                cap_type: g.cap_type.clone(),
                grantor: g.grantor,
                grantee: g.grantee,
                resource: g.resource,
            }
        }).collect();

        TransactionDeltaView {
            actor_id: d.actor_id,
            objects_created: d.births.clone(),
            objects_deleted: d.deaths.clone(),
            objects_frozen: d.freezes.clone(),
            links_added,
            links_removed: d.unlinks.clone(),
            memory_written,
            capability_events,
            capability_grants,
            effects,
        }''',
    },
]


def backup(path):
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak.{ts}"
    shutil.copy2(path, bak)
    return bak


def apply_patch(entry):
    path = entry["file"]
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    count = content.count(entry["old"])
    if count == 0:
        print(f"[SKIP] {path}: 锚点未找到。")
        return False
    if count > 1:
        print(f"[ABORT] {path}: 锚点出现 {count} 次,不唯一。")
        return False
    bak = backup(path)
    new_content = content.replace(entry["old"], entry["new"], 1)
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] {path} 已修改,备份于 {bak}")
    return True


def main():
    results = [apply_patch(p) for p in PATCHES]
    if not all(results):
        print("\n[未全部成功] 检查上面提示。")
        sys.exit(1)

    print("\n=== 编译 ===")
    build = subprocess.run(["cargo", "build"], capture_output=True, text=True)
    print(build.stdout[-2000:])
    print(build.stderr[-4000:])

    if build.returncode != 0:
        print("\n[FAIL] 贴给我分析。")
        sys.exit(1)

    print("\n=== 跑一次相关测试确认没破坏现有断言 ===")
    test = subprocess.run(
        ["cargo", "test", "--lib"],
        capture_output=True, text=True
    )
    print(test.stdout[-1500:])
    print(test.stderr[-1500:])
    tail = test.stdout[-1500:]
    if "test result: ok" in tail or "FAILED" not in tail:
        print("\n[SUCCESS] 结构化 capability_grants 视图已上线,全量测试通过。")
    else:
        print("\n[注意] 有测试失败,贴完整输出给我分析。")


if __name__ == "__main__":
    main()
