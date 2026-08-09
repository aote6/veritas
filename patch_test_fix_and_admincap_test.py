#!/usr/bin/env python3
"""
1. 修复 tx_write 调用缺少第4参数(object_id: Option<ObjectId>)导致的编译错误
2. 新增 create_object_short 的 capability 归属测试(填补此前发现的测试空白)
"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCHES = [
    {
        "file": "./src/world_api.rs",
        "old": '''        world
            .tx_write(sid, 0, b"/tmp/test.txt".to_vec())
            .expect("write state_id=0 must succeed on newly created object");
        world
            .tx_write(sid, 1, b"hello".to_vec())
            .expect("write state_id=1 must succeed on newly created object");''',
        "new": '''        world
            .tx_write(sid, 0, b"/tmp/test.txt".to_vec(), None)
            .expect("write state_id=0 must succeed on newly created object");
        world
            .tx_write(sid, 1, b"hello".to_vec(), None)
            .expect("write state_id=1 must succeed on newly created object");''',
    },
]

NEW_TEST = '''
    /// Regression: create_object_short must return a valid AdminCap that the
    /// creator actually holds. Prior to this fix, the capability was granted
    /// internally (WAL-recorded) but silently dropped at the API boundary —
    /// callers only ever received the ObjectId, never the CapabilityId.
    #[test]
    fn create_object_short_returns_valid_admin_cap() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id, admin_cap) = world
            .create_object_short()
            .expect("create_object_short must succeed");

        assert!(
            kernel.engine().holds_capability(admin_cap, id),
            "creator object must hold the AdminCap returned by create_object_short"
        );
    }

    /// Regression: an object with no capability on a resource must be
    /// rejected — this is the inverse case that was previously untested
    /// (only the "has permission -> succeeds" path was covered).
    #[test]
    fn object_without_capability_is_denied() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id_a, _cap_a) = world
            .create_object_short()
            .expect("create_object_short must succeed for object A");
        let (id_b, cap_b) = world
            .create_object_short()
            .expect("create_object_short must succeed for object B");

        // id_a must NOT hold id_b's AdminCap — they are independently granted.
        assert!(
            !kernel.engine().holds_capability(cap_b, id_a),
            "object A must not hold object B's AdminCap"
        );
    }
'''


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


def find_test_mod_insertion_point(path):
    """在文件末尾的最后一个 `}` 前插入新测试(假设文件以 mod tests 的闭合括号结尾)。"""
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    idx = content.rfind("}")
    if idx == -1:
        return None, content
    return idx, content


def insert_new_test(path):
    idx, content = find_test_mod_insertion_point(path)
    if idx is None:
        print(f"[ABORT] {path}: 找不到插入点(文件里没有 '}}')。")
        return False
    if "create_object_short_returns_valid_admin_cap" in content:
        print(f"[SKIP] {path}: 新测试似乎已存在,跳过插入。")
        return True
    bak = backup(path)
    new_content = content[:idx] + NEW_TEST + "\n" + content[idx:]
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] 新测试已插入 {path},备份于 {bak}")
    return True


def main():
    print("=== 修复 tx_write 参数缺失 ===")
    results = [apply_patch(p) for p in PATCHES]
    if not all(results):
        print("\n[未全部成功] 请检查上面提示。")
        sys.exit(1)

    print("\n=== 插入新增 capability 测试 ===")
    if not insert_new_test("./src/world_api.rs"):
        sys.exit(1)

    print("\n=== 编译 + 运行测试 ===")
    build = subprocess.run(
        ["cargo", "test", "--lib", "create_object_short"],
        capture_output=True, text=True
    )
    print(build.stdout[-4000:])
    print(build.stderr[-4000:])

    if build.returncode == 0:
        print("\n[SUCCESS] 测试通过。")
    else:
        print("\n[FAIL] 上面是完整输出,贴给我分析。")
        sys.exit(1)


if __name__ == "__main__":
    main()
