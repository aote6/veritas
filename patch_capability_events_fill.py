#!/usr/bin/env python3
"""
补丁四: TransactionDeltaView.capability_events 从硬编码空数组改为
实际填充 d.capability_grants 的内容 —— 之前是设计了字段但实现里从未填充。
"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCH = {
    "file": "./src/world_api.rs",
    "old": '''        TransactionDeltaView {
            actor_id: d.actor_id,
            objects_created: d.births.clone(),
            objects_deleted: d.deaths.clone(),
            objects_frozen: d.freezes.clone(),
            links_added,
            links_removed: d.unlinks.clone(),
            memory_written,
            capability_events: vec![],
            effects,
        }''',
    "new": '''        let capability_events: Vec<String> = d.capability_grants.iter().map(|g| {
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
}


def main():
    path = PATCH["file"]
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()

    count = content.count(PATCH["old"])
    if count == 0:
        print("[SKIP] 锚点未找到,手动检查。")
        sys.exit(1)
    if count > 1:
        print(f"[ABORT] 锚点出现 {count} 次,不唯一。")
        sys.exit(1)

    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak.{ts}"
    shutil.copy2(path, bak)
    new_content = content.replace(PATCH["old"], PATCH["new"], 1)
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] 已修改,备份于 {bak}")

    print("\n=== 编译 ===")
    build = subprocess.run(["cargo", "build"], capture_output=True, text=True)
    print(build.stdout[-2000:])
    print(build.stderr[-3000:])

    if build.returncode != 0:
        print("\n[FAIL] 编译错误,贴给我分析,不要继续。")
        sys.exit(1)

    print("\n[SUCCESS] 编译通过。")
    print("\n注意: capability_events 目前只输出人类可读字符串(和字段类型 Vec<String> 一致),")
    print("forge 端如果需要程序化解析 cap_id,建议之后把 TransactionDeltaView 里")
    print("再加一个结构化字段(如 capability_grants: Vec<CapabilityGrantView>),")
    print("而不是靠 forge 反解析字符串 —— 这个决定先不动,等你确认 forge 具体怎么用再定。")


if __name__ == "__main__":
    main()
