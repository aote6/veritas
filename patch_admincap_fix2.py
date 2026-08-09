#!/usr/bin/env python3
"""
补丁二:修复第一版遗留的两个编译错误
  1. world_api.rs 缺少 CapabilityId 导入
  2. world_api.rs:233 ensure_identity() 里的调用点未跟着解构新返回值
"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCHES = [
    {
        "file": "./src/world_api.rs",
        "old": '''use crate::types::{
    AbortReason, LinkSnapshot, LinkType, ObjectId, ObjectState, ObjectType, StateId,
    TransactionContext, TransactionDelta, TransactionReceipt, VeritasError, Version,
};''',
        "new": '''use crate::types::{
    AbortReason, CapabilityId, LinkSnapshot, LinkType, ObjectId, ObjectState, ObjectType,
    StateId, TransactionContext, TransactionDelta, TransactionReceipt, VeritasError, Version,
};''',
    },
    {
        "file": "./src/world_api.rs",
        "old": '''        let id = self.create_object_short()?;
        *self.identity.lock().unwrap() = Some(id);
        Ok(id)
    }''',
        "new": '''        let (id, _admin_cap) = self.create_object_short()?;
        *self.identity.lock().unwrap() = Some(id);
        Ok(id)
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
        print(f"[SKIP] {path}: 锚点未找到,可能已修改过,手动检查下方 diff 目标文本。")
        print("----- 期望找到的文本 -----")
        print(entry["old"])
        print("--------------------------")
        return False
    if count > 1:
        print(f"[ABORT] {path}: 锚点出现 {count} 次,不唯一,拒绝自动替换。")
        return False

    bak = backup(path)
    new_content = content.replace(entry["old"], entry["new"], 1)
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] {path} 已修改,备份于 {bak}")
    return True


def main():
    print("=== 打补丁二 ===")
    results = [apply_patch(p) for p in PATCHES]
    if not all(results):
        print("\n[未全部成功] 检查上面 SKIP/ABORT,不要继续编译。")
        sys.exit(1)

    print("\n=== 编译验证 (cargo build) ===")
    build = subprocess.run(["cargo", "build"], capture_output=True, text=True)
    print(build.stdout[-3000:])
    print(build.stderr[-4000:])

    if build.returncode == 0:
        print("\n[SUCCESS] 编译通过。")
        print("接下来建议跑一次全量测试,确认没有其他调用点被漏掉:")
        print("  cargo test --lib create_object_short 2>&1 | tail -50")
        print("  cargo build --bin veritasd")
    else:
        print("\n[FAIL] 仍有编译错误,把上面完整输出贴给我分析。")
        sys.exit(1)


if __name__ == "__main__":
    main()
