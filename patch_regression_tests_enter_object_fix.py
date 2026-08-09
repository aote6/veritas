#!/usr/bin/env python3
"""
补丁八(收尾): 为 CRITICAL 补丁(tx_write/tx_freeze_object/tx_death_object
的 enter_object 越权修复)补三条专属回归测试。防止未来重构时这三处检查
被意外删除,又回到 2026-08-09 发现的越权状态。
"""
import shutil
import subprocess
import sys
from datetime import datetime

NEW_TEST = '''
    /// Regression for CRITICAL fix (2026-08-09): tx_write with a foreign
    /// object_id must NOT be able to bypass the capability graph by forcing
    /// enter_object() before authorization. Object A without any capability
    /// on Object B must be denied. (This is the write-path sibling of
    /// cross_object_access_still_requires_capability, kept separate to
    /// pin the specific historical bug: prior to the fix, enter_object was
    /// called unconditionally, making target == current_object trivially
    /// true post-switch and defeating authorize_intent entirely.)
    #[test]
    fn tx_write_cross_object_without_capability_denied() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id_a, _cap_a) = world.create_object_short().unwrap();
        let (id_b, _cap_b) = world.create_object_short().unwrap();

        let sid = world.tx_begin(Some(id_a)).unwrap();
        let result = world.tx_write(sid, 0, b"forged".to_vec(), Some(id_b));
        assert!(
            result.is_err(),
            "A must not be able to write into B's MemorySpace without a capability on B"
        );
        // current_object must NOT have been switched to B as a side effect
        // of the denied attempt — a rejected authorization must not leave
        // the session mid-switched.
    }

    /// Regression for CRITICAL fix (2026-08-09): tx_freeze_object must not
    /// allow an unauthorized object to freeze another object. Freeze is
    /// irreversible (Alive -> Frozen per object.md lifecycle), so this is
    /// higher stakes than a plain read/write violation.
    #[test]
    fn tx_freeze_object_cross_object_without_capability_denied() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id_a, _cap_a) = world.create_object_short().unwrap();
        let (id_b, _cap_b) = world.create_object_short().unwrap();

        let sid = world.tx_begin(Some(id_a)).unwrap();
        let result = world.tx_freeze_object(sid, id_b);
        assert!(
            result.is_err(),
            "A must not be able to freeze B without a capability on B"
        );
        assert_ne!(
            kernel.get_object_state(id_b),
            Some(ObjectState::Frozen),
            "B must remain unfrozen after a denied freeze attempt"
        );
    }

    /// Regression for CRITICAL fix (2026-08-09): tx_death_object must not
    /// allow an unauthorized object to kill another object. Death is
    /// irreversible and cascades OWNS links, making this the highest-stakes
    /// of the three fixed entry points.
    #[test]
    fn tx_death_object_cross_object_without_capability_denied() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id_a, _cap_a) = world.create_object_short().unwrap();
        let (id_b, _cap_b) = world.create_object_short().unwrap();

        let sid = world.tx_begin(Some(id_a)).unwrap();
        let result = world.tx_death_object(sid, id_b);
        assert!(
            result.is_err(),
            "A must not be able to kill B without a capability on B"
        );
        assert_eq!(
            kernel.get_object_state(id_b),
            Some(ObjectState::Alive),
            "B must remain Alive after a denied death attempt"
        );
    }

    /// Regression: legitimate self-freeze (object freezing itself) must
    /// still work after the CRITICAL fix — confirms the fix did not
    /// collateral-damage the structural self-access exemption for
    /// lifecycle operations, not just read/write.
    #[test]
    fn tx_freeze_object_self_still_allowed() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let (id, _cap) = world.create_object_short().unwrap();
        let sid = world.tx_begin(Some(id)).unwrap();
        world
            .tx_freeze_object(sid, id)
            .expect("object must still be able to freeze itself after the fix");
        world.tx_commit(sid).expect("commit of self-freeze must succeed");
        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Frozen));
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
    if "tx_write_cross_object_without_capability_denied" in content:
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

    print("\n=== 编译 + 全量测试 ===")
    result = subprocess.run(["cargo", "test", "--lib"], capture_output=True, text=True)
    print(result.stdout[-4000:])
    print(result.stderr[-2000:])

    if "FAILED" in result.stdout or result.returncode != 0:
        print("\n[FAIL] 有测试失败或编译错误,贴完整输出给我分析。")
        sys.exit(1)

    print("\n[SUCCESS] 全部通过。今天的安全修复已经被四条专属回归测试锁定。")


if __name__ == "__main__":
    main()
