# Veritas Kernel 快速上手与 CLI 操作文档

本文档汇总了 Veritas Kernel 从汇编编写、模块编译、虚拟机运行到 WAL 状态持久化与状态查询的完整标准流程。

---

## 1. 汇编语法规则 (.vasm)

编写 `.vasm` 源文件时，请遵循以下指令规范：

- **模块定义**：必须包含 `module <名称>` 与 `version <x.y.z>`。
- **注释与段落**：注释用 `;` 开头；无需写 `.code` 或 `.module` 等带点的标号。
- **对象创建 (OBJECT_BIRTH)**：格式为 `OBJECT_BIRTH <dummy_id>`。内核会动态分配全局唯一 ID 并自动写入寄存器 `R0`。
- **对象关联 (OBJECT_LINK)**：格式为 `OBJECT_LINK <from>, <to>, <relation>`。
  参数可为寄存器（如 `R0`, `R1`）或具体 ID。
  关系类型 (`relation`) 支持：`owns`、`depends_on`、`references`。
- **事务与终止**：提交变更使用 `COMMIT`，结束虚拟机执行使用 `HALT`。

---

## 2. 标准操作流程

### 步骤一：准备汇编文件

以创建两个相互依赖的世界对象为例，创建 `world_demo.vasm`：

```asm
module world_demo
version 1.0.0

OBJECT_BIRTH 0
LOAD_CONST R2, 0
ADD R1, R0, R2
OBJECT_BIRTH 0
OBJECT_LINK R1, R0, owns
OBJECT_LINK R0, R1, depends_on
COMMIT
HALT
```

### 步骤二：编译汇编代码为字节码模块 (.vmod)

```bash
cargo run --bin veritas -- compile world_demo.vasm world_demo.vmod
```

### 步骤三：初始化 WAL 文件并运行虚拟机

注意：`run` 命令要求 WAL 持久化文件必须提前存在（可先通过 `touch` 创建）：

```bash
touch ./world_demo.wal
cargo run --bin veritas -- run world_demo.vmod ./world_demo.wal
```

### 步骤四：查看持久化状态与拓扑关系

1. **列出 WAL 中的所有活跃对象**：
   ```bash
   cargo run --bin veritas -- inspect ./world_demo.wal list
   ```

2. **查询指定对象的内核状态**：
   ```bash
   cargo run --bin veritas -- inspect ./world_demo.wal object 1
   ```

---

## 3. 常见问题排查

| 现象 / 报错 | 根因说明 | 解决办法 |
| :--- | :--- | :--- |
| `Missing module name` | 汇编文件中使用了 `.module` 或误加入了未支持的指示符 | 去掉前导点，直接使用 `module <name>` |
| `Bad num: R0` | `OBJECT_BIRTH` 参数填了 `R0` | `OBJECT_BIRTH` 参数填任意数字即可，分配结果会自动存入 `R0` |
| `Os { code: 2, ... NotFound }` | 指定的 WAL 文件不存在 | 执行前先运行 `touch <wal_path>` |
| `Error: unknown inspect subcommand` | 使用了未定义的 inspect 子命令 | inspect 仅支持 `list` 和 `object <id>` |

---

## 4. 全量回归测试

在更新内核或修改指令集后，可运行完整测试套件：

```bash
cargo test
```
