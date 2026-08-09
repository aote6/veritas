#!/usr/bin/env python3
"""
CRITICAL 安全补丁: tx_write / tx_freeze_object / tx_death_object 三处
在调用 enter_object(target) 切换执行身份之前,没有做任何权限校验 ——
任何 session 可以无条件切换成系统内任意 ObjectId 的身份,进而绕过
authorize_intent 的检查(因为切换后 target == current_object,豁免条件
天然成立)。这不是"缺测试",是真实的、可复现的权限穿透漏洞:
  - tx_write:  越权读写任意对象的 MemorySpace
  - tx_freeze_object: 越权冻结任意对象(不可逆)
  - tx_death_object:  越权杀死任意对象(不可逆)

修复方式: 在 enter_object 之前,先用 AccessIntent::Call(target) 走一次
authorize_intent 检查。只有当前 actor 对 target 持有 capability,或者
target 就是当前 actor 自己(结构性豁免),才允许切换身份继续执行。
"""
import shutil
import subprocess
import sys
from datetime import datetime

PATCHES = [
    {
        "file": "./src/world_api.rs",
        "old": '''    pub fn tx_write(
        &self,
        session_id: SessionId,
        state_id: u64,
        payload: Vec<u8>,
        object_id: Option<ObjectId>,
    ) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            if let Some(oid) = object_id {
                if state.ctx.current_object != oid {
                    state.ctx.enter_object(oid);
                }
            }
            kernel.write(&mut state.ctx, state_id, payload)?;
            Ok(())
        })
    }''',
        "new": '''    pub fn tx_write(
        &self,
        session_id: SessionId,
        state_id: u64,
        payload: Vec<u8>,
        object_id: Option<ObjectId>,
    ) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            if let Some(oid) = object_id {
                if state.ctx.current_object != oid {
                    // SECURITY: must authorize the cross-object context switch
                    // BEFORE enter_object, otherwise target == current_object
                    // becomes trivially true post-switch and the capability
                    // graph is bypassed entirely.
                    kernel.engine().authorize_intent(
                        &state.ctx,
                        &crate::types::AccessIntent::Call(oid),
                    )?;
                    state.ctx.enter_object(oid);
                }
            }
            kernel.write(&mut state.ctx, state_id, payload)?;
            Ok(())
        })
    }''',
    },
    {
        "file": "./src/world_api.rs",
        "old": '''    pub fn tx_freeze_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            // Self-freeze requires acting as the target (AccessIntent).
            if state.ctx.current_object != object_id {
                state.ctx.enter_object(object_id);
            }
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectFreeze { object_id },
            )?;
            Ok(())
        })
    }''',
        "new": '''    pub fn tx_freeze_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            // Self-freeze requires acting as the target (AccessIntent).
            if state.ctx.current_object != object_id {
                // SECURITY: authorize before switching context — freeze is
                // irreversible (Alive -> Frozen), must not be triggerable
                // by an unauthorized caller via bare object_id parameter.
                kernel.engine().authorize_intent(
                    &state.ctx,
                    &crate::types::AccessIntent::Call(object_id),
                )?;
                state.ctx.enter_object(object_id);
            }
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectFreeze { object_id },
            )?;
            Ok(())
        })
    }''',
    },
    {
        "file": "./src/world_api.rs",
        "old": '''    pub fn tx_death_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            if state.ctx.current_object != object_id {
                state.ctx.enter_object(object_id);
            }
            kernel.handle(''',
        "new": '''    pub fn tx_death_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            if state.ctx.current_object != object_id {
                // SECURITY: authorize before switching context — death is
                // irreversible and cascades OWNS links, must not be
                // triggerable by an unauthorized caller.
                kernel.engine().authorize_intent(
                    &state.ctx,
                    &crate::types::AccessIntent::Call(object_id),
                )?;
                state.ctx.enter_object(object_id);
            }
            kernel.handle(''',
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
        print(f"[SKIP] {path}: 锚点未找到,可能格式和预期不完全一致,手动检查。")
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
        print("\n[未全部成功] 请检查上面 SKIP/ABORT 提示,贴给我看具体哪处不匹配。")
        sys.exit(1)

    print("\n=== 编译 ===")
    build = subprocess.run(["cargo", "build"], capture_output=True, text=True)
    print(build.stdout[-2000:])
    print(build.stderr[-4000:])

    if build.returncode != 0:
        print("\n[FAIL] 编译错误。可能 engine() 是 pub(crate) 但访问路径不对,")
        print("或者 authorize_intent 签名和预期不同。把完整错误贴给我。")
        sys.exit(1)

    print("\n=== 全量测试 (确认没有破坏合法场景,比如 self-freeze / self-death) ===")
    test = subprocess.run(["cargo", "test", "--lib"], capture_output=True, text=True)
    print(test.stdout[-3000:])
    print(test.stderr[-1500:])

    if "FAILED" in test.stdout:
        print("\n[注意] 有测试失败 —— 需要看是不是合法的 self-freeze/self-death")
        print("场景被误伤(比如 target 本来就是 actor 自己,但 authorize_intent")
        print("对 Call 的豁免判断和 Write/Destroy/Freeze 不一致导致误拒)。")
        print("贴完整输出给我分析,不要放宽检查让测试通过。")
    else:
        print("\n[SUCCESS] 编译通过,全量测试通过。三处越权入口已堵上。")
        print("\n下一步强烈建议: 补三条专属回归测试,证实修复前的攻击路径")
        print("现在确实被拒绝(A 越权 write/freeze/death B 必须失败)。")


if __name__ == "__main__":
    main()
