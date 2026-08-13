#!/usr/bin/env python3
"""Update commit_version.md section 3.3 with frozen canonical identity encoding"""

import re

with open("docs/constitution/commit_version.md", "r", encoding="utf-8") as f:
    content = f.read()

# 替换第 3.3 节
old_33 = """### 3.3 delta_content_hash 的定义

delta_content_hash = Hash(canonical_serialize(TransactionDelta))

必须排除的字段:

commit_version 是位置，不是身份

tx_id 是过程标识，可伪造

WAL wrapper 是传输层元数据

CRC 是传输层完整性校验

before_root 和 after_root 是结果，不是身份

必须固定的编码规则:

字段顺序固定

collection 顺序固定，HashMap 必须转换为 BTreeMap 排序后序列化

enum 编码固定

数值采用固定 endian，规定为 little-endian

字符串字节长度明确
"""

new_33 = """### 3.3 delta_content_hash 的定义

delta_content_hash = Hash(canonical_identity_bytes(delta))

canonical_identity_bytes 是独立于 WAL serialize() 的二进制编码，专门服务 Delta Identity。

必须排除的字段:

commit_version 是位置，不是身份

tx_id 是过程标识，可伪造

WAL wrapper 是传输层元数据

CRC 是传输层完整性校验

before_root 和 after_root 是结果，不是身份

必须包含的字段:

actor_id 是提交语义的一部分，进入 Delta Identity

writes 顺序敏感，按 Vec 原始顺序编码

scope_changes 顺序敏感，按 Vec 原始顺序编码

births 顺序敏感，按 Vec 原始顺序编码

deaths 顺序敏感，按 Vec 原始顺序编码

freezes 顺序敏感，按 Vec 原始顺序编码

links 顺序敏感，按 Vec 原始顺序编码

unlinks 顺序敏感，按 Vec 原始顺序编码

capability_grants 顺序敏感，按 Vec 原始顺序编码

capability_delegates 顺序敏感，按 Vec 原始顺序编码

capability_revokes 顺序敏感，按 Vec 原始顺序编码

effects 顺序敏感，按 Vec 原始顺序编码

canonical_identity_bytes 必须满足的编码规则:

第一条: 排除 commit_version

第二条: 排除 tx_id

第三条: 包含 actor_id

第四条: 保持所有 Vec 字段的精确顺序

第五条: enum 变体使用显式分配的稳定标签编码

第六条: 整数使用固定宽度 little-endian binary 编码，u64 为 8 字节，u32 为 4 字节

第七条: 可变长度字节和字符串使用显式长度前缀编码，格式为 u64 长度加原始字节

第八条: 排除 WAL 传输元数据、CRC、before_root、after_root

第九条: 不依赖 Rust Debug 或 Display 格式化

第十条: 语义完全相同的 TransactionDelta 必须产生完全相同的字节序列

字段编码顺序固定为:

ACTOR

WRITES

SCOPE_CHANGES

BIRTHS

DEATHS

FREEZES

LINKS

UNLINKS

CAPABILITY_GRANTS

CAPABILITY_DELEGATES

CAPABILITY_REVOKES

EFFECTS

以后新增字段不得插入已有顺序中间，只能追加到末尾，否则会破坏 identity encoding 的兼容性。

架构关系:

TransactionDelta 同时服务两个独立职责:

第一个职责是 WAL serialization，继续使用现有 serialize() 文本格式，保持人类可读和现有 replay 兼容

第二个职责是 canonical_identity_bytes()，使用二进制长度前缀编码，专门计算 content_hash

两者互不影响，不得让现有 serialize() 同时承担 identity 规范职责。
"""

content = content.replace(old_33, new_33)

with open("docs/constitution/commit_version.md", "w", encoding="utf-8") as f:
    f.write(content)

print("Updated section 3.3")
print(f"Total lines: {content.count(chr(10)) + 1}")
