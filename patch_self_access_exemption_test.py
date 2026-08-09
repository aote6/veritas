#!/usr/bin/env python3
"""
补丁七: 新增自身对象访问豁免(self-access exemption)专属测试。
对应宪法条款:"Object 访问自己的 MemorySpace 不经 Capability 图,是结构性内建权限"。
实现已确认存在于 engine.rs:authorize_intent (target == current_object 时 continue),
但此前没有专属测试锁定这个行为 —— 本补丁填补这个空白。
"""
import shutil
import subprocess
import sys
from datetime import datetime

NEW_TEST = '''
    /// Regression: an object must be able to read/write its own MemorySpace
    /// with ZERO capabilities held — this is the constitutional "structural
    /// exemption" (engine.rs authorize_intent: target == current_object ||
    /// target == capability_context => skip capability graph entirely).
    /// Prior to this test, the exemption existed in code but was never
    /// directly exercised by any test — only indirectly implied.
    #[test]
    fn self_access_bypasses_capability_graph() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id, _admin_cap) = world
            .create_object_short()
            .expect("create_object_short must succeed");

        // Open a session acting AS this object. Do not attach or use the
        // AdminCap at all — if self-access required capability, this would
        // fail even though the object legitimately owns this memory.
        let sid = world.tx_begin(Some(id)).unwrap();
        world
            .tx_write(sid, 0, b"self-written".to_vec(), None)
            .expect("object must be able to write its own MemorySpace without any capability");
        let value = world
            .tx_read(sid, 0)
            .expect("object must be able to read its own MemorySpace without any capability");
        assert_eq!(value, b"self-written".to_vec());
        world.tx_commit(sid).expect("commit of self-access-only tx must succeed");
    }

    /// Companion to self_access_bypasses_capability_graph: confirms the
    /// exemption is narrowly scoped to self, not a blanket bypass. An
    /// object with no capability on ANOTHER object must still be denied —
    /// this mirrors object_without_capability_is_denied but is placed here
    /// to make the self-vs-other boundary explicit and co-located.
    #[test]
    fn cross_object_access_still_requires_capability() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id_a, _cap_a) = world.create_object_short().unwrap();
        let (id_b, _cap_b) = world.create_object_short().unwrap();

        // A acts, but tries to write into B's memory without ever having
        // been granted a capability on B. Must fail.
        let sid = world.tx_begin(Some(id_a)).unwrap();
        let result = world.tx_write(sid, 0, b"intrusion".to_vec(), Some(id_b));
        assert!(
            result.is_err(),
            "object A must not be able to write into object B's MemorySpace without a capability"
        );
    }
'''


def find_test_mod_insertion_point(path):
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    idx = content.rfind("}")
    if idx == -1:
        return None, content
    return idx, content


def insert_new_test(path):
    idx, content = find_test_mod_insertion_point(path)
    if idx is None:
        print(f"[ABORT] {path}: 找不到插入点。")
        return False
    if "self_access_bypasses_capability_graph" in content:
        print(f"[SKIP] {path}: 测试似乎已存在,跳过。")
        return True
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak.{ts}"
    shutil.copy2(path, bak)
    new_content = content[:idx] + NEW_TEST + "\n" + content[idx:]
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] 新测试已插入 {path},备份于 {bak}")
    return True


def main():
    if not insert_new_test("./src/world_api.rs"):
        sys.exit(1)

    print("\n=== 编译 + 运行新测试 ===")
    result = subprocess.run(
        ["cargo", "test", "--lib",
         "self_access_bypasses_capability_graph",
         "--", "--test-threads=1"],
        capture_output=True, text=True
    )
    # cargo test with multiple filter args doesn't work that way; run separately below instead.
    result2 = subprocess.run(
        ["cargo", "test", "--lib", "cross_object_access_still_requires_capability"],
        capture_output=True, text=True
    )
    print("--- self_access_bypasses_capability_graph ---")
    print(result.stdout[-2000:])
    print(result.stderr[-1500:])
    print("--- cross_object_access_still_requires_capability ---")
    print(result2.stdout[-2000:])
    print(result2.stderr[-1500:])

    ok1 = "test result: ok" in result.stdout
    ok2 = "test result: ok" in result2.stdout
    if ok1 and ok2:
        print("\n[SUCCESS] 两条测试都通过 —— 自身豁免语义和跨对象隔离语义都已被锁定。")
    else:
        print("\n[FAIL] 至少一条未通过,贴完整输出给我分析。若 self-access 测试失败,")
        print("说明豁免逻辑在 tx_write/tx_read 这条路径上实际没生效(可能只在别的调用路径生效)。")


if __name__ == "__main__":
    main()
