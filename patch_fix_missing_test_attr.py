#!/usr/bin/env python3
"""补丁三: session_abort_discards 缺失 #[test] 属性,补上并验证"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCH = {
    "file": "./src/world_api.rs",
    "old": '''    fn session_abort_discards() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(kernel);
        let sid = world.tx_begin(None).unwrap();
        let id = world.tx_create_object(sid).unwrap();
        world.tx_abort(sid).unwrap();
        assert!(world.get_object(id).is_none());
    }''',
    "new": '''    #[test]
    fn session_abort_discards() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(kernel);
        let sid = world.tx_begin(None).unwrap();
        let id = world.tx_create_object(sid).unwrap();
        world.tx_abort(sid).unwrap();
        assert!(world.get_object(id).is_none());
    }''',
}


def main():
    path = PATCH["file"]
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()

    count = content.count(PATCH["old"])
    if count == 0:
        print("[SKIP] 锚点未找到,可能已经改过,手动检查。")
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
    print(f"[OK] 已添加 #[test],备份于 {bak}")

    print("\n=== 运行该测试 ===")
    result = subprocess.run(
        ["cargo", "test", "--lib", "session_abort_discards"],
        capture_output=True, text=True
    )
    print(result.stdout[-3000:])
    print(result.stderr[-2000:])

    if result.returncode == 0 and "test result: ok" in result.stdout:
        print("\n[SUCCESS] 测试补上并通过 —— abort 语义在当前架构下依然成立。")
    else:
        print("\n[注意] 测试补上但未通过,或输出异常。这说明 abort 逻辑")
        print("在架构重构后可能真的坏掉了,而不是测试代码的问题。")
        print("把上面完整输出贴给我分析,不要自行修改断言让它'通过'。")


if __name__ == "__main__":
    main()
