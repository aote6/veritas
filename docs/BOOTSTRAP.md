# Veritas 自举能力分析报告

最后更新: 2026-08-10

本文档分析 Veritas 当前的自举能力现状，以及完成完整自举还需要补充什么。

---

## 1. 什么是自举

在 Veritas 语境下，自举 = Veritas 能跑在 Veritas 上。

具体分 6 个阶段：

| 阶段 | 含义 | 当前状态 |
|------|------|---------|
| Bootstrap 1 | 用 Rust 编译器编译 Veritas 内核 | ✅ 能（cargo build） |
| Bootstrap 2 | Veritas 能编译 Veritas 汇编代码（.vasm） | ✅ 能（assembler.rs） |
| Bootstrap 3 | Veritas 能执行 Veritas 编译出的程序 | ✅ 能（machine.rs） |
| Bootstrap 4 | Veritas 能加载并执行自己的汇编器 | ❌ 不能 |
| Bootstrap 5 | Veritas 能修改自身代码并生效 | ❌ 不能 |
| Bootstrap 6 | Veritas 能重启自身而不丢失状态 | ❌ 不能 |

现状：Bootstrap 1-3 已通，Bootstrap 4-6 未通。

---

## 2. 硬件对照表

| Veritas 概念 | 计算机硬件对应 |
|-------------|--------------|
| World State | 硬盘/ROM（关机后还在的东西） |
| Transaction | CPU 执行的一串指令（临时状态） |
| ObjectRegistry | 文件系统的 inode 表（记录有哪些文件） |
| StateStore | 硬盘上的数据块（文件内容） |
| Topology | 文件系统的目录树（谁是谁的父目录） |
| CapabilityGraph | 访问控制列表 ACL（谁能访问哪个文件） |
| global_version | 系统启动计数器（检测配置是否被改过） |
| object_id_counter | 硬盘序列号生成器（下一个新硬盘编号多少） |
| grant_sequence | 证书序列号生成器（下一张证书编号多少） |
| Kernel API | 系统调用（syscall）（用户程序请求内核干活） |
| ModuleInstance | 进程/线程（代码在内存里跑起来） |
| Effect | 打印机输出/网络发包（计算结果输出到外部） |
| Receipt 链 | 区块链的区块（世界状态变化的证明链） |

---

## 3. 当前缺什么（6 项）

### 3.1 缺：程序加载器（Program Loader）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| BIOS/UEFI 加载操作系统 | Veritas 只能跑编译好的 .vmod | Veritas 需要从文件系统加载任意 .vmod |

现在缺什么：
- 没有文件系统，不能存/取程序文件
- 没有程序加载器，不能动态加载新代码

自举需要：
- 一个简单的文件系统（存 .vmod 文件）
- 一个加载器指令（LOAD_MODULE <path>）
- 加载后能跳到新模块执行

---

### 3.2 缺：自修改能力（Self-Modification）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| 操作系统内核升级 | Object 创建后不可改代码 | Veritas 需要能修改自己的代码段 |

现在缺什么：
- ModuleObject 是只读的（宪法规定：创建后默认 FROZEN）
- 不能修改已加载的模块代码

自举需要：
- 允许 FROZEN 对象在特定条件下解冻
- 或者设计代码热更新机制
- 或者用新对象替换旧对象实现升级

---

### 3.3 缺：无限循环/常驻进程（Infinite Loop / Resident Process）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| CPU 的无限循环能力 | 程序执行完 HALT 就停 | Veritas 需要能一直运行不停止 |

现在缺什么：
- 所有程序都是跑完就停（HALT 或 COMMIT 结束）
- 没有常驻进程概念

自举需要：
- 一个永远运行的进程（类似操作系统的 init 进程）
- 或者调度器，让多个程序轮流跑

---

### 3.4 缺：运行时动态入口（Runtime Dynamic Entry）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| Linux 的 execve() | 只有编译时确定的标签跳转 | Veritas 需要运行时决定执行哪个程序 |

现在缺什么：
- CALL 只能跳到编译时确定的标签（@entry）
- 不能运行时决定我跳转到哪个对象的哪个入口

自举需要：
- CALL 的 entry_pc 支持运行时动态值（当前 entry_pc 是编译时确定的 usize）
- 或者增加 EXEC 指令：EXEC <module_id>, <entry_name>

---

### 3.5 缺：环境/参数传递（Environment / Arguments）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| 命令行参数 argc/argv | 没有参数传递 | Veritas 需要给程序传参数 |

现在缺什么：
- 所有程序硬编码行为（没有外部输入）
- 不能告诉程序你要编译哪个文件

自举需要：
- 程序启动时能接收参数
- 类似 main(argc, argv) 或环境变量

---

### 3.6 缺：持久化自举状态（Persistent Bootstrap State）

| 硬件对应 | 当前状态 | 自举需要 |
|---------|---------|---------|
| BIOS 保存启动顺序 | 每次启动从头开始 | Veritas 需要记住上次自举到哪里了 |

现在缺什么：
- 系统每次启动都是全新开始
- 没有启动进度的概念

自举需要：
- 某个对象记录当前自举阶段
- 启动时读取这个状态，继续而不是从头开始

---

## 4. 当前宪法缺口（7 项）

| 缺什么 | 硬件对应 | 没有的影响 | 加上的变化 |
|--------|---------|-----------|----------|
| global_version 等不进快照 | 硬盘序列号计数器 | ID 可能重用/冲突 | 恢复后 ID 永远唯一 |
| StateEntry.version 硬编码 | 磁盘扇区版本号 | 不能做 MVCC | 支持多版本并发控制 |
| Kernel API 未收敛 | CPU syscall | 用户程序可改内核 | 强制通过内核调用 |
| verify_capability 覆盖不全 | MMU 权限检查 | 可以读别人的数据 | 全操作权限可控 |
| ModuleInstance 未实现 | 进程/线程 | 代码跑不起来 | 多模块独立执行 |
| Effect 模型待定 | 外设输出 | 计算结果出不去 | 可输出到外部 |
| root_hash u64 | MD5 签名 | 可能被伪造 | SHA-256 可信验证 |

---

## 5. 完整自举路线图

| 阶段 | 需要的硬件能力 | 当前状态 | 需要补什么 |
|:---:|--------------|:-------:|-----------|
| 1 | 编译 Veritas 内核（Rust） | ✅ | — |
| 2 | 汇编 .vasm -> .vmod | ✅ | — |
| 3 | 执行 .vmod | ✅ | — |
| 4 | 加载任意 .vmod | ❌ | 文件系统 + 加载器 |
| 5 | 运行时决定执行哪个入口 | ❌ | 动态 entry_pc 或 EXEC 指令 |
| 6 | 给程序传参数 | ❌ | 参数传递机制 |
| 7 | 程序永远运行（不停止） | ❌ | 调度器 / 常驻进程 |
| 8 | 修改自身代码并生效 | ❌ | 代码热更新 |
| 9 | 重启后继续自举进度 | ❌ | 持久化自举状态 |

自举完成 = 阶段 1-9 全部通。

---

## 6. 总结

Veritas 现在是一台能跑完一个程序就关机的单片机。

要完成自举，需要变成能加载程序、传参数、常驻运行、自我升级的完整计算机。

当前缺 6 样东西：

1. 文件系统（存程序文件）
2. 程序加载器（动态加载 .vmod）
3. 动态入口（运行时决定执行哪个入口）
4. 参数传递（给程序传外部输入）
5. 调度器/常驻进程（程序永远运行）
6. 代码热更新（修改自身代码并生效）

加上这 6 样，再补上宪法层面的 7 个缺口，Veritas 就能完成完整自举。

---

## 7. 相关文档

- docs/constitution/world.md — World State Constitution v1.0
- docs/constitution/kernel.md — Kernel Service Interface v0.2
- docs/constitution/object.md — Object Specification v0.2
- docs/IDENTITY_MODEL.md — 执行身份与授权模型说明
- STATUS.md — 项目整体状态
