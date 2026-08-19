#!/usr/bin/env python3
"""Update lagging docs for 2026-08-19."""

# 1. STATUS.md - append at end
with open("STATUS.md", "a") as f:
    f.write("\n## 2026-08-19 P30.4/P30.5/P30.6 HostCall/MemoryAlloc/dead_code + Forge E2E 验证\n\n")
    f.write("**背景**：上一轮（2026-08-17）完成 capability_grants 序列化修复后，继续清理无设计歧义的技术债，并验证 Forge 集成完整性。\n\n")
    f.write("**完成内容**：\n")
    f.write("- P30.4: HostCall 枚举统一（src/host.rs: Time/Random/Write/Read/Spawn）\n")
    f.write("- P30.5: MemoryAlloc 真实实现（engine.memory_alloc → allocated_slots 事务内跟踪，不污染 StateStore）\n")
    f.write("- P30.6: dead_code 清理（test-only/bootstrap 方法标注）\n")
    f.write("- checkpoint 注释修正（TEST-ONLY → production infrastructure）\n\n")
    f.write("**Forge E2E 验证**：\n")
    f.write("- 手动启动 veritasd + JSONL 协议：ping → attach_identity → tx_begin → tx_create_object → tx_write → tx_read → tx_commit → list_objects → world_info\n")
    f.write("- 全链路通过：身份分配（object_id=1）、对象创建（object_id=2）、AdminCap 自动授予、WAL 恢复（2 条记录）\n")
    f.write("- Receipt 完整性确认：before_root → after_root 变化正确，delta 含 memory_written/objects_created/capability_grants\n\n")
    f.write("**验证**：\n")
    f.write("- cargo test：365 passed, 0 failed\n")
    f.write("- 新增测试：host.rs 单元测试（4个）、memory_alloc_sequential_state_ids（含空值污染验证）\n\n")
    f.write("**相关 commit**：\n")
    f.write("- 6eba3e7 P30.4-P30.6: HostCall enum, MemoryAlloc initial impl, dead_code cleanup\n")
    f.write("- 4f19dc2 chore: remove patch artifacts\n")
    f.write("- 85e4184 fix: MemoryAlloc no empty-value pollution + checkpoint comments corrected\n")

# 2. ROADMAP_NEXT.md
with open("ROADMAP_NEXT.md") as f:
    content = f.read()

content = content.replace("最后更新: 2026-08-16", "最后更新: 2026-08-19")
content = content.replace("cargo test: 360 passed", "cargo test: 365 passed")
content = content.replace("- MemoryAlloc：KernelCall 到达即空操作\n", "")
content = content.replace("## 已完成", "## 已完成\n\n### P30.4-P30.6 技术债清理 ✅（2026-08-19）\n- HostCall 枚举统一（src/host.rs）\n- MemoryAlloc 真实实现（allocated_slots 不污染 StateStore）\n- dead_code 清理 + checkpoint 注释修正\n- Forge E2E 全链路验证通过")

with open("ROADMAP_NEXT.md", "w") as f:
    f.write(content)

# 3. TEST_ARCHITECTURE.md
with open("docs/TEST_ARCHITECTURE.md") as f:
    content = f.read()

content = content.replace("| L0 | `src/**` `#[test]` | 103 |", "| L0 | `src/**` `#[test]` | 129 |")
content = content.replace("| L0 单元测试 | 103 个，冻结 |", "| L0 单元测试 | 129 个，冻结 |")
content = content.replace("| Verification Map | 236/236，CI 强制 |", "| Verification Map | 245/245，CI 强制 |")

with open("docs/TEST_ARCHITECTURE.md", "w") as f:
    f.write(content)

# 4. VASM_EXECUTION_MODEL.md
with open("docs/VASM_EXECUTION_MODEL.md") as f:
    content = f.read()

content = content.replace(
    "## 8. 已知问题：Runtime::execute 遇到 Trap 会死循环",
    "## 8. 已修复：Runtime::execute 遇到 Trap 会死循环（2026-08-14 修复）"
)
content = content.replace(
    "修复思路（未实施）：把 is_halted() 的 matches! 里加上 MachineStatus::Trapped(_)，",
    "**状态更新（2026-08-14）**：已修复。`is_halted()` 已加入 `MachineStatus::Trapped(_)` 匹配。\n\n修复思路（已实施）：把 is_halted() 的 matches! 里加上 MachineStatus::Trapped(_)，"
)

with open("docs/VASM_EXECUTION_MODEL.md", "w") as f:
    f.write(content)

# 5. USAGE.md - append
with open("docs/USAGE.md", "a") as f:
    f.write("\n---\n\n## 5. veritasd JSONL 接口（Forge）\n\n")
    f.write("启动：\n```bash\nVERITAS_WAL=./world.wal ./target/debug/veritasd\n```\n\n")
    f.write("命令示例（每行一个 JSON）：\n\n")
    f.write("| 命令 | 示例 | 响应 |\n")
    f.write("|------|------|------|\n")
    f.write("| ping | `{\"cmd\":\"ping\"}` | `{\"ok\":true,\"result\":\"pong\"}` |\n")
    f.write("| attach_identity | `{\"cmd\":\"attach_identity\"}` | `{\"object_id\":1,\"ok\":true}` |\n")
    f.write("| whoami | `{\"cmd\":\"whoami\"}` | `{\"object_id\":1,\"ok\":true}` |\n")
    f.write("| tx_begin | `{\"cmd\":\"tx_begin\"}` | `{\"ok\":true,\"session_id\":1}` |\n")
    f.write("| tx_create_object | `{\"cmd\":\"tx_create_object\",\"session_id\":1}` | `{\"object_id\":2,\"ok\":true}` |\n")
    f.write("| tx_write | `{\"cmd\":\"tx_write\",\"session_id\":1,\"state_id\":0,\"value\":\"/hello.txt\"}` | `{\"ok\":true}` |\n")
    f.write("| tx_read | `{\"cmd\":\"tx_read\",\"session_id\":1,\"state_id\":1}` | `{\"ok\":true,\"value_hex\":\"...\"}` |\n")
    f.write("| tx_commit | `{\"cmd\":\"tx_commit\",\"session_id\":1}` | `{\"ok\":true,\"receipt\":{...}}` |\n")
    f.write("| list_objects | `{\"cmd\":\"list_objects\"}` | `{\"objects\":[{\"id\":1,\"state\":\"Alive\"}],\"ok\":true}` |\n")
    f.write("| world_info | `{\"cmd\":\"world_info\"}` | `{\"object_count\":N,\"state_root\":\"...\",\"version\":N}` |\n")

print("Done.")
