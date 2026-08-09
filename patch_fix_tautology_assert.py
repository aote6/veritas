#!/usr/bin/env python3
"""补丁六: 修复 session_multi_op_commit 里的永真式断言(|| true 使其恒真,形同虚设)"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCH = {
    "file": "./src/world_api.rs",
    "old": "        assert!(receipt.after_root != 0 || receipt.before_root != receipt.after_root || true);",
    "new": "        assert_ne!(receipt.before_root, receipt.after_root, \"commit must change root hash\");",
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

    print("\n=== 运行该测试,确认修真断言后依然通过 ===")
    result = subprocess.run(
        ["cargo", "test", "--lib", "session_multi_op_commit"],
        capture_output=True, text=True
    )
    print(result.stdout[-2500:])
    print(result.stderr[-1500:])

    if result.returncode == 0 and "test result: ok" in result.stdout:
        print("\n[SUCCESS] 修真后依然通过 —— root hash 在 commit 前后确实发生变化,底层逻辑没问题。")
    else:
        print("\n[注意] 修真断言后测试失败 —— 说明 root hash 在这个场景下")
        print("实际没有变化,之前的 || true 掩盖了一个真实的行为异常。")
        print("把完整输出贴给我分析,不要把断言改回宽松版本让它'通过'。")


if __name__ == "__main__":
    main()
