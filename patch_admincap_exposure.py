#!/usr/bin/env python3
"""
Patch: 让 create_object_short 把 AdminCap 一起返回给调用方
影响文件:
  - src/world_api.rs   (create_object_short 签名+实现)
  - src/bin/veritasd.rs (create_object 分支适配新返回值)
"""
import shutil
import subprocess
import sys
from datetime import datetime

ROOT = "."

PATCHES = [
    {
        "file": f"{ROOT}/src/world_api.rs",
        "old": '''    pub fn create_object_short(&self) -> Result<ObjectId, WorldError> {
        let mut ctx = self.kernel.begin();
        let result = self.kernel.handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )?;
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => return Err(WorldError::Msg("ObjectBirth did not return ObjectId".into())),
        };
        let _receipt = self.kernel.commit(&mut ctx)?;
        Ok(id)
    }''',
        "new": '''    pub fn create_object_short(&self) -> Result<(ObjectId, CapabilityId), WorldError> {
        let mut ctx = self.kernel.begin();
        let result = self.kernel.handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )?;
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => return Err(WorldError::Msg("ObjectBirth did not return ObjectId".into())),
        };
        let receipt = self.kernel.commit(&mut ctx)?;

        let admin_cap = receipt
            .delta
            .capability_grants
            .iter()
            .find(|g| g.grantee == id && g.resource == id)
            .map(|g| g.capability_id)
            .ok_or_else(|| WorldError::Msg("AdminCap not found after ObjectBirth".into()))?;

        Ok((id, admin_cap))
    }''',
    },
    {
        "file": f"{ROOT}/src/bin/veritasd.rs",
        "old": '''        "create_object" => match world.create_object_short() {
            Ok(id) => json!({"ok": true, "object": {"id": id, "state": "Alive"}}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },''',
        "new": '''        "create_object" => match world.create_object_short() {
            Ok((id, admin_cap)) => json!({
                "ok": true,
                "object": {"id": id, "state": "Alive", "admin_cap_id": admin_cap}
            }),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },''',
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
        print(f"[SKIP] {path}: 锚点文本未找到,可能已经改过或代码有变化,手动检查。")
        return False
    if count > 1:
        print(f"[ABORT] {path}: 锚点文本出现 {count} 次,不唯一,拒绝自动替换,手动处理。")
        return False

    bak = backup(path)
    new_content = content.replace(entry["old"], entry["new"], 1)
    with open(path, "w", encoding="utf-8") as f:
        f.write(new_content)
    print(f"[OK] {path} 已修改,备份于 {bak}")
    return True


def main():
    print("=== 检查其他 create_object_short 调用点(防止漏改) ===")
    grep = subprocess.run(
        ["grep", "-rn", "create_object_short(", "src/"],
        capture_output=True, text=True
    )
    print(grep.stdout)
    call_sites = [l for l in grep.stdout.splitlines() if "fn create_object_short" not in l]
    if len(call_sites) > 1:
        print(f"[警告] 发现 {len(call_sites)} 处调用(不含定义本身),"
              f"本脚本只改了 veritasd.rs 一处,其余需要你手动确认返回值解构是否要跟着改:")
        for l in call_sites:
            print("   ", l)
    print()

    print("=== 开始打补丁 ===")
    results = [apply_patch(p) for p in PATCHES]
    if not all(results):
        print("\n[未全部成功] 请检查上面的 SKIP/ABORT 提示,不要继续 cargo build。")
        sys.exit(1)

    print("\n=== 编译验证 (cargo build) ===")
    build = subprocess.run(["cargo", "build"], capture_output=True, text=True)
    print(build.stdout[-3000:])
    print(build.stderr[-3000:])

    if build.returncode == 0:
        print("\n[SUCCESS] 编译通过。AdminCap 现在会随 create_object 响应一起返回。")
        print("下一步:forge 端 forge/forge/adapters/veritas_adapter.py 需要接住并存储这个 admin_cap_id,")
        print("这部分我们另外再处理,不在本脚本范围内。")
    else:
        print("\n[FAIL] 编译未通过,上面是错误输出。")
        print("补丁本身已落盘(备份文件在同目录 *.bak.<时间戳>),")
        print("如需回滚,对每个 *.bak.<时间戳> 文件执行:")
        print("  cp <备份文件> <原文件路径>")
        sys.exit(1)


if __name__ == "__main__":
    main()
