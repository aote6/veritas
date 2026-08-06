# Veritas 指令集（草案）

状态：Draft，尚未进入 Constitution

## 当前实现中的指令

| 指令 | 类别 | 说明 |
|------|------|------|
| LOAD  | Memory | 从 (ObjectId, StateId) 读取 |
| STORE | Memory | 写入 (ObjectId, StateId) |
| CALL  | Control | 跨 Object 调用 |
| RETURN| Control | 返回调用者 |
| TRAP  | Kernel | 进入内核态 |
| JMP   | Control | 无条件跳转 |
| MOV   | Register| 寄存器间移动 |
| ADD   | ALU | 加法 |

## 未来方向

ISA 最终应进入 Constitution（isa.md），但需等指令集稳定后。
当前阶段指令集仍可能演化，过早宪法化会增加修改成本。
