# Veritas Kernel 快速上手与 CLI 操作文档

本文档汇总了 Veritas Kernel 从汇编编写、模块编译、虚拟机运行到 WAL 状态持久化与状态查询的完整标准流程。

---

## 1. 汇编语法规则 (.vasm)

编写 `.vasm` 源文件时，请遵循以下指令规范：

- **模块定义**：必须包含 `module <名称>` 与 `version <x.y.z>`。
- **注释与段落**：注释用 `;` 开头；无需写 `.code` 或 `.module` 等带点的标号。
- **Kernel service（唯一入口 TRAP）**：`TRAP <service_id>`。例如 `TRAP 0` 创建对象（ObjectId → R0），`TRAP 5` 提交事务。
  对象链接：先把 from/to/link_type 写入 R0/R1/R2（link_type：0=depends_on，1=owns，2=references），再 `TRAP 2`。
  完整 service_id 表见 `docs/Veritas_指令集.md` / `docs/TRAP_ABI_FREEZE.md`。
  **已退役助记符**：`OBJECT_BIRTH`、`OBJECT_LINK`、`COMMIT` 等，Assembler 会拒绝。
- **终止**：`HALT` 结束虚拟机执行。

---

## 2. 标准操作流程

### 步骤一：准备汇编文件

以创建两个相互依赖的世界对象为例，创建 `world_demo.vasm`：

```asm
module world_demo
version 1.0.0

TRAP 0
LOAD_CONST R2, 0
ADD R1, R0, R2
TRAP 0
LOAD_CONST R4, 0
ADD R3, R0, R4
ADD R0, R1, R4
ADD R1, R3, R4
LOAD_CONST R2, 1
TRAP 2
TRAP 5
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
| `Unknown op: OBJECT_BIRTH` | 使用了已退役的 Kernel mnemonic | 改用 `TRAP 0`（及对应 service_id），见指令集文档 |
| `Os { code: 2, ... NotFound }` | 指定的 WAL 文件不存在 | 执行前先运行 `touch <wal_path>` |
| `Error: unknown inspect subcommand` | 使用了未定义的 inspect 子命令 | inspect 仅支持 `list` 和 `object <id>` |

---

## 4. 全量回归测试

在更新内核或修改指令集后，可运行完整测试套件：

```bash
cargo test
```

---

## 5. veritasd JSONL 接口（Forge）

启动：
```bash
VERITAS_WAL=./world.wal ./target/debug/veritasd
```

命令示例（每行一个 JSON）：

| 命令 | 示例 | 响应 |
|------|------|------|
| ping | `{"cmd":"ping"}` | `{"ok":true,"result":"pong"}` |
| attach_identity | `{"cmd":"attach_identity"}` | `{"object_id":1,"ok":true}` |
| whoami | `{"cmd":"whoami"}` | `{"object_id":1,"ok":true}` |
| tx_begin | `{"cmd":"tx_begin"}` | `{"ok":true,"session_id":1}` |
| tx_create_object | `{"cmd":"tx_create_object","session_id":1}` | `{"object_id":2,"ok":true}` |
| tx_write | `{"cmd":"tx_write","session_id":1,"state_id":0,"value":"/hello.txt"}` | `{"ok":true}` |
| tx_read | `{"cmd":"tx_read","session_id":1,"state_id":1}` | `{"ok":true,"value_hex":"..."}` |
| tx_commit | `{"cmd":"tx_commit","session_id":1}` | `{"ok":true,"receipt":{...}}` |
| list_objects | `{"cmd":"list_objects"}` | `{"objects":[{"id":1,"state":"Alive"}],"ok":true}` |
| world_info | `{"cmd":"world_info"}` | `{"object_count":N,"state_root":"...","version":N}` |
