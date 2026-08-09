#!/usr/bin/env python3
"""更新 STATUS.md 并提交推送本次审查+修复的全部工作"""
import subprocess
import sys
from datetime import datetime

ENTRY = """
## 2026-08-09 安全审查与修复(capability 暴露 + 越权漏洞)

**背景:** 对 forge↔veritas 打通阶段的宪法合规性做例行审查,聚焦五个已知缺口
(Capability 绕过、Transaction 边界、自身对象豁免、DependencyInvalidated 监听、
测试严谨性)。审查过程中发现多处"实现存在但未生效"的隐蔽债务,以及一个
CRITICAL 级别的真实越权漏洞。

### 修复清单

1. **`create_object_short` 静默丢弃 AdminCap(已修复)**
   `world_api.rs` — commit 后从 `receipt.delta.capability_grants` 中提取
   AdminCap,返回类型从 `Result<ObjectId, _>` 改为
   `Result<(ObjectId, CapabilityId), _>`。同步修正 `veritasd.rs` 的
   `create_object` action 响应,新增 `admin_cap_id` 字段。

2. **`TransactionDeltaView.capability_events` 硬编码空数组(已修复)**
   `from_delta()` 中 `capability_events: vec![]` 从未被真正填充。现改为从
   `d.capability_grants` 生成人类可读日志,并新增结构化字段
   `capability_grants: Vec<CapabilityGrantView>` 供程序化读取
   capability_id/grantor/grantee/resource,不再需要解析字符串。

3. **`session_abort_discards` 测试缺失 `#[test]` 属性(已修复)**
   函数体逻辑正确但从未被执行过。补上属性后验证通过,abort 语义在当前
   架构下依然成立。

4. **`session_multi_op_commit` 中永真式断言(已修复)**
   `assert!(... || true)` 使断言恒真、形同虚设。修正为
   `assert_ne!(receipt.before_root, receipt.after_root)`,验证后依然通过,
   证实 root hash 逻辑本身没有问题(此前只是被这行掩盖,未被验证过)。

5. **【CRITICAL】`tx_write` / `tx_freeze_object` / `tx_death_object` 越权漏洞(已修复)**
   三处在调用 `enter_object(target)` 切换执行身份前,没有做任何权限校验:
   任意 session 可以无条件将 `current_object` 切换为系统内任意 `ObjectId`,
   切换后 `target == current_object` 天然成立,直接绕过
   `authorize_intent` 的 capability 检查。
   影响面:越权读写任意对象 MemorySpace、越权冻结任意对象(不可逆)、
   越权杀死任意对象(不可逆,级联 OWNS)。
   修复:三处在 `enter_object` 之前,先以 `AccessIntent::Call(target)`
   走一次 `authorize_intent`,未授权则直接拒绝、不切换身份。
   发现路径:为验证"自身对象访问豁免"(#4)补充对照测试时,
   `cross_object_access_still_requires_capability` 测试意外失败,
   顺藤摸瓜定位到 `enter_object` 本身零校验 + 三处外部 API 直接拿
   调用方传入的裸 `object_id` 触发切换。

### 新增测试(共 9 条)
`create_object_short_returns_valid_admin_cap`、
`object_without_capability_is_denied`、
`self_access_bypasses_capability_graph`、
`cross_object_access_still_requires_capability`、
`tx_write_cross_object_without_capability_denied`、
`tx_freeze_object_cross_object_without_capability_denied`、
`tx_death_object_cross_object_without_capability_denied`、
`tx_freeze_object_self_still_allowed`、
以及修复已存在的 `session_abort_discards`。
全量测试:98 passed, 0 failed。

### 结论 / 后续
- 内核核心(kernel.rs/engine.rs)本身的 capability 校验逻辑是可信的
  (`verify_capability`/`authorize_intent` 设计正确);本次问题集中在
  **对外暴露的 API 层**没有一致地复用这套校验。
- Transaction 边界(原判断为#2缺口)经核实**已完整存在**
  (session API: begin/create/freeze/death/link/unlink/write/read/commit/abort
  全部实现且已透传至 veritasd),此前的缺口评估是过时信息,已划掉。
- 自身对象访问豁免(#4)语义已确认在 `authorize_intent` 中正确实现
  (`target == current_object || target == capability_context` 时豁免),
  现有专属测试锁定。
- forge 端 `adapter.py` 仍未接住新暴露的 `admin_cap_id`——有意搁置,
  待后续专项处理。
- DependencyInvalidated 监听(#5)仍未实现——forge 目前只做
  create_object,未触发 Link/Death,暂无法测试,继续记债。
"""


def main():
    with open("STATUS.md", "a", encoding="utf-8") as f:
        f.write(ENTRY)
    print("[OK] STATUS.md 已追加本次记录")

    print("\n=== git add ===")
    subprocess.run(["git", "add", "-A"], check=True)

    print("=== git status (确认将要提交的内容) ===")
    status = subprocess.run(["git", "status", "--short"], capture_output=True, text=True)
    print(status.stdout)

    commit_msg = (
        "fix(security): patch enter_object authorization bypass in tx_write/freeze/death\n\n"
        "CRITICAL: tx_write, tx_freeze_object, tx_death_object called enter_object()\n"
        "on caller-supplied object_id without prior authorization, allowing any\n"
        "session to switch current_object to an arbitrary target and trivially\n"
        "satisfy authorize_intent's self-access exemption. Fixed by checking\n"
        "AccessIntent::Call(target) via authorize_intent before switching.\n\n"
        "Also fixes:\n"
        "- create_object_short silently dropped AdminCap (now returns it)\n"
        "- TransactionDeltaView.capability_events was hardcoded empty (now filled,\n"
        "  plus new structured capability_grants field)\n"
        "- session_abort_discards test was missing #[test] attribute (never ran)\n"
        "- session_multi_op_commit had a tautological assertion (|| true)\n\n"
        "Adds 9 tests covering capability grant/denial, self-access exemption,\n"
        "and the cross-object authorization bypass specifically.\n"
        "Full suite: 98 passed, 0 failed."
    )

    print("\n=== git commit ===")
    result = subprocess.run(
        ["git", "commit", "-m", commit_msg],
        capture_output=True, text=True
    )
    print(result.stdout)
    print(result.stderr)

    if result.returncode != 0:
        print("[注意] commit 可能没有变更(比如 STATUS.md 已经写过),检查上面输出。")
        sys.exit(1)

    print("\n=== git push ===")
    push = subprocess.run(["git", "push"], capture_output=True, text=True)
    print(push.stdout)
    print(push.stderr)

    if push.returncode == 0:
        print("\n[SUCCESS] STATUS.md 已更新,提交并推送完成。")
    else:
        print("\n[FAIL] push 失败,把上面完整输出贴给我 —— 可能是网络、认证或者需要先 pull。")
        sys.exit(1)


if __name__ == "__main__":
    main()
