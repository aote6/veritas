# Veritas 机器原语判别准则 v0.1

## 0. 什么是 Veritas

Veritas 不是在复制物理硬件。Veritas 是在回答一个更根本的问题：

**如果软件世界也有自己的硬件，那么最小机器到底应该长什么样。**

它不是虚拟机、不是数据库、不是操作系统、不是把已有软件概念搬到一起。
它是软件世界自己的 CPU——有自己的寄存器、MMU、特权态、中断、原子指令，
但这些东西的形态由软件世界的需求决定，不由硅决定。

## 1. 三层过滤

任何功能在进入 Constitution 之前，必须通过三层过滤：

### 第一问：它在现实硬件里的对应物是什么？

要求给出精确对应，不是为了模仿硬件，而是为了确认这个抽象属于"机器"而非"库"：

| 硬件对应物 | Veritas 形态 | 为什么是机器 |
|-----------|-------------|------------|
| 寄存器 | TransactionContext 中的执行状态 | ISA 可见，指令操作的对象 |
| MMU | (ObjectId, StateId) 寻址 | 地址隔离，不是 HashMap key |
| 页表 | Capability + MemorySpace | 回答"一个地址属于谁" |
| 特权态/中断 | Kernel 模式 + TRAP | 用户程序无法直接拥有的特权执行区 |
| 原子指令 | Transaction | 软件计算机保证状态一致性的最小机器周期 |

### 第二问：如果现实硬件没有对应物，它为什么必须成为 Veritas 的机器原语？

允许出现硬件不存在但 Veritas 必须有的原语。但必须给出理由。

**Transaction 是唯一的现行案例：**
CPU 没有"事务"这个概念。但 Veritas 作为软件世界自己的硬件，
必须有一个比 Load/Store 更大粒度的状态一致性保证机制。
Transaction 就是这台机器的"原子指令"——它是 Veritas 新增的硬件能力。

### 第三问：删除它，这台软件计算机还能成立吗？

- **不能，机器模型崩塌** → 属于 Constitution
- **能，只是慢一些** → 属于未来优化（如 Cache、Pipeline），不进 Constitution
- **能，只是方便一些** → 属于 Runtime / Library / Host / Tool / Compiler，不进 Constitution

### 被明确过滤的

| 概念 | 为什么不属于 Constitution |
|------|--------------------------|
| Cache | 没有也能工作，只是慢 |
| Pipeline | 8086 没有，一样能跑 |
| 便利 API | 编译器/宿主的事，不是机器的事 |
| 设计模式 | 软件思维，不是硬件思维 |
| 数据库事务语义 | Transaction 是机器周期，不是 ACID |

## 2. 为什么需要这条元准则

Constitution 是 Veritas 的不变量——不可绕过、不可违反的运行时强制规则。

如果没有元准则，任何觉得"重要"的东西都可能被塞进 Constitution，
最终变成另一个软件框架。

这条准则强制区分：
- **机器**（没有它不行）→ Constitution
- **优化**（没有它慢）→ 工程层
- **便利**（没有它麻烦）→ 工具层

## 3. 当前 Constitution 的机器原语清单

| 原语 | 硬件对应 | 地位 |
|------|---------|------|
| Object | 可寻址实体（进程/保护域） | 一等机器原语 |
| MemorySpace | 地址空间（MMU 管理的区域） | 一等机器原语 |
| Address = (ObjectId, StateId) | 虚拟地址 | 一等机器原语 |
| Capability | 页表项（谁可以访问什么） | 一等机器原语 |
| Kernel + TRAP | 特权态 + 中断 | 一等机器原语 |
| Transaction | Veritas 新增硬件能力（状态一致性最小周期） | 一等机器原语 |
| Link | 总线连接（组件间命名关系） | 一等机器原语 |
| Module | 可执行代码段 | 一等机器原语 |

## 4. 与 Constitution 其他文件的关系

本文件是 Constitution 的**第0号文件**——元准则。

所有其他 Constitution 文件（kernel.md, object.md, memory.md,
transaction.md, link.md, module.md）必须符合本准则。

当出现争议时，三层过滤是最终仲裁依据。
