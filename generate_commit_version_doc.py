#!/usr/bin/env python3
"""Generate docs/constitution/commit_version.md"""

content = """# Veritas Commit Version & Delta Identity Constitution v0.1

最后更新: 2026-08-13
状态: 已冻结，待实现

## 1. 宪法地位

本文档补充 world.md 中关于 global_version 的定义，明确 commit_version 的语义、Delta Identity 的构成、以及 apply() 的准入状态机。

本文档与 world.md、transaction.md、object.md、memory.md、kernel.md、link.md 同级，是 Veritas Machine State 完整性的核心不变量。

## 2. 核心不变量

For every committed world state, global_version and last_applied_delta_hash jointly identify the terminal committed Delta of that world.

A Delta may advance the world only when its commit_version is exactly global_version + 1.

A Delta at the current version is a no-op only when its canonical content hash equals last_applied_delta_hash; otherwise it is a history conflict and must be rejected without mutation.

## 3. 核心定义

### 3.1 commit_version 是什么

commit_version 是 World State 的线性提交序号，不是 Delta 的唯一身份。

它是 global_version 在某个 Delta 成功 apply 后的新值。

正常提交必须满足: commit_version = current_global_version + 1

版本序列是严格连续的: 0 → 1 → 2 → 3 → ...

不允许跳跃: 0 → 1 → 5 是非法的

### 3.2 Delta Identity 是什么

Delta Identity 是一个元组: (commit_version, delta_content_hash)

commit_version 定位这个 Delta 在世界演化序列中的位置。

delta_content_hash 保证同版本不同内容能被区分。

tx_id 不是 Delta Identity 的一部分。它是过程内标识，可伪造，不属于 World State，不参与身份判定。

### 3.3 delta_content_hash 的定义

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

### 3.4 World State 新增字段

global_version: u64

last_applied_delta_hash: Hash

这两个字段是不可分割的 World State 元组，必须一起进入 WorldSnapshot、Checkpoint、root_hash 计算。

不能出现 version = N 但 hash = H(N-1) 的状态。

## 4. 初始状态定义

当 global_version = 0 时，还没有任何 Delta 被 apply。

规定:

global_version = 0

last_applied_delta_hash = ZERO_HASH

ZERO_HASH 是固定定义的全零哈希值。

对应关系:

version 0 + ZERO_HASH

接受 Delta(version=1)

变为:

version 1 + hash(delta1)

整个状态机从 genesis 开始就是闭合的。

## 5. apply() 准入状态机

incoming Delta 进入 apply() 后，按以下顺序判定:

第一步: version < current

一律 REJECT，不管 hash 是什么。

第二步: version == current

若 hash == last_applied_delta_hash，则 NO-OP，世界不变。

若 hash != last_applied_delta_hash，则 REJECT，世界不变。

第三步: version == current + 1

APPLY，正常推进，同时更新 global_version 和 last_applied_delta_hash。

第四步: version > current + 1

REJECT，版本跳跃是非法的。

## 6. 不变量

### 6.1 版本唯一性

每个 commit_version 只能对应一个 Delta content。一旦某个版本被 apply，该版本的 content 就永久固定。

### 6.2 版本连续性

合法提交必须是连续的。从任何一致的世界状态出发，下一个合法 Delta 的 commit_version 必须是 current + 1。

### 6.3 幂等重放

完全相同的内容在同一版本重复出现，必须是无害的 no-op。这是 Recovery 和 Replay 多次执行的基础。

### 6.4 冲突拒绝

相同版本、不同内容必须拒绝，且不能产生任何状态变更。这是防止历史改写的关键。

### 6.5 拒绝原子性

REJECT 路径必须在任何 mutation 之前返回。不能出现先改了一半再拒绝的情况。

### 6.6 幂等根哈希

NO-OP 路径必须保证 apply 前后 root_hash 完全一致。

## 7. 与现有宪法的一致性

world.md 定义 global_version 是 World State，本宪法补充 last_applied_delta_hash 也是 World State，两者共同构成版本身份的完整定义。

transaction.md 明确 tx_id 不属于 Machine State，本宪法规定 tx_id 不参与 Delta Identity，完全一致。

transaction.md 第十三节要求确定性，本宪法要求 delta_content_hash 基于确定性序列化，完全一致。

memory.md 要求版本号单调递增，本宪法要求 commit_version 严格连续递增，是同一原则在更高层的体现。

object.md 要求 ObjectId 不可重用，本宪法要求 commit_version 同样不可重用，是同一原则在不同实体上的应用。

DETERMINISTIC_WORLD.md 要求 Replay 统一走 TransactionDelta 到 apply()，本宪法的准入状态机保证了 Replay 的安全性。

## 8. 实现约束

### 8.1 不引入新字段到 TransactionDelta

当前 TransactionDelta 已有确定性 serialization，直接计算 hash 即可。不需要给 Delta 添加新的持久化字段。

### 8.2 apply() 保持单一入口

状态机在 apply() 内部实现，Runtime commit 和 Recovery replay 都走同一路径，禁止第二套投影逻辑。

### 8.3 拒绝对世界的任何变更

REJECT 路径必须在任何 mutation 之前返回。

### 8.4 持久化要求

last_applied_delta_hash 必须与 global_version 一起进入 WorldSnapshot 和 Checkpoint。重启后必须能够恢复这个字段，否则 restart 边界不闭合。

## 9. 测试矩阵

测试必须覆盖以下九种情况:

Case 1: current=0, incoming=1, hash=new, 预期 APPLY

Case 2: current=1, incoming=2, hash=new, 预期 APPLY

Case 3: current=2, incoming=2, hash=same, 预期 NO-OP

Case 4: current=2, incoming=2, hash=different, 预期 REJECT

Case 5: current=2, incoming=1, hash=any, 预期 REJECT

Case 6: current=2, incoming=4, hash=new, 预期 REJECT

Case 7: 重复连续 WAL 0到1到2, 第二次重放 1到2, 预期 NO-OP

Case 8: reject 原子性, current=N, incoming=非法, 预期 World 完全不变

Case 9: NO-OP 根哈希, current=N, incoming=N, hash=same, 预期 root 完全不变

现有的 audit_equal_version_residual_gap_red 应该拆分为两个测试:

audit_equal_version_same_content_is_idempotent

audit_equal_version_different_content_is_rejected

## 10. 实施顺序

第一步: 审计 TransactionDelta 的 canonical serialization 是否满足 3.3 节的所有要求

第二步: 检查 Checkpoint 和 WorldSnapshot 是否能恢复 last_applied_delta_hash

第三步: 确认第一步和第二步都成立后，修改 apply() 实现状态机

第四步: 运行完整测试矩阵

第五步: cargo test --all 全量回归

第六步: WAL adversarial audit 验证 restart 边界闭合

第一步和第二步不可跳过。如果 canonical serialization 不满足要求，或者 checkpoint 不能恢复 hash，本宪法在 restart 边界就不闭合，必须先修复这两个基础条件。

## 11. 与现有红测试的关系

现有的 audit_equal_version_residual_gap_red 测试证明了 equal-version different-payload 会被错误接受。

本宪法将这个红测试升级为完整的语义测试，明确定义了 equal-version 的两种分支:

同内容 → 幂等 no-op

不同内容 → 冲突拒绝

这解决了 residual gap，同时保留了合法的幂等 replay 能力。

## 12. 设计哲学

本宪法不是给 Veritas 添加新的机制，而是恢复一个缺失的机器寄存器。

这台离硬件层最近的计算机，原本就应该有一个记录最后提交指令指纹的寄存器。

之前实现时漏掉了这个寄存器，导致 equal-version 的语义无法被正确判断。

现在补上，是修复，不是添加。
"""

import os

output_path = "docs/constitution/commit_version.md"

with open(output_path, "w", encoding="utf-8") as f:
    f.write(content)

print(f"Generated: {output_path}")
print(f"Lines: {content.count(chr(10)) + 1}")
