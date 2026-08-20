# Veritas 指令集

最后更新: 2026-08-20  
状态: Machine 指令集 + TRAP Kernel service ABI（旧式 Kernel mnemonic 已退役）

## 操作数 (Operand)

指令操作数可以是立即数或寄存器:

- 立即数: 十进制(10)或十六进制(0x1A)
- 寄存器: R0-R7

设计原则: 任何可能由前序指令在运行时产生的值，必须使用 Operand 而非裸数值类型。

## 寄存器

8 个通用寄存器 R0–R7。R0 同时是 TRAP / 部分原生指令的返回值寄存器。  
标志位: zero, negative, overflow, carry。

---

## 一、计算与控制流

| 助记符 | 说明 |
|--------|------|
| NOP | 空操作 |
| LOAD_CONST Rn, imm | 加载立即数到寄存器 |
| ADD Rd, Rs1, Rs2 | Rd = Rs1 + Rs2 |
| SUB Rd, Rs1, Rs2 | Rd = Rs1 - Rs2 |
| CMP Rs1, Rs2 | 比较，设置 zero/negative |
| JMP target | 无条件跳转 |
| JZ target | zero=1 时跳转 |
| JNZ target | zero=0 时跳转 |
| JN target | negative=1 时跳转 |
| CALL object_id, pc | 跨对象调用，切换 current_object（须 AccessIntent::Call） |
| RETURN | 从 CALL 返回 |
| HALT | 停机 |

## 二、状态读写（Machine 原生，非 TRAP）

| 助记符 | 说明 |
|--------|------|
| READ state_id | 读取状态字节到 R0 |
| WRITE state_id, "string" | 写入字符串到状态 |
| LOAD_STATE_U64 Rn, sid | 加载状态中的 u64 |
| LOAD_STATE_BYTES Rn, sid | 加载状态字节 |
| WRITE_REGISTER sid, Rn | 寄存器值写入状态（state_id 仅立即数） |

WRITE 的 state_id 支持寄存器操作数。

## 三、Kernel service：唯一入口 TRAP

**已退役（Assembler 拒绝）**：`OBJECT_BIRTH`、`OBJECT_DEATH`、`OBJECT_LINK`、`OBJECT_UNLINK`、`OBJECT_FREEZE`、`COMMIT`、`EFFECT`、`SAVEPOINT`、`ROLLBACK_TO`、`CAPABILITY_GRANT`、`ABORT`。

正式写法：

```
TRAP <service_id>
```

参数经 R0/R1/R2，或 R0 指向 RAM 中 little-endian **参数块**（复杂服务）。  
解码：`KernelCall::decode_with_memory` → `Kernel::handle` → Engine。  
完整布局见 `docs/TRAP_ABI_FREEZE.md`。

| service_id | KernelCall | 参数 ABI | 典型返回 |
|------------|------------|----------|----------|
| 0 | ObjectBirth | R0=object_type（0=State，非0=Module） | ObjectId → R0；并 attach self-AdminCap |
| 1 | ObjectDeath | R0=object_id | Success |
| 2 | ObjectLink | R0=from, R1=to, R2=link_type（0 depends_on / 1 owns / 2 references） | Success |
| 3 | ObjectUnlink | R0=from, R1=to | Success |
| 4 | ObjectFreeze | R0=object_id | Success |
| 5 | Commit | 无 | Success |
| 6 | Effect | R0=参数块地址 | EffectKey → R0（UTF-8 bytes） |
| 7 | Savepoint | R0=参数块（name UTF-8） | Success |
| 8 | RollbackTo | 同 Savepoint | Success |
| 9 | CapabilityGrant | R0=参数块（grantor/grantee/type/resource） | CapabilityId → R0 |
| 10 | CapabilityRevoke | R0=参数块 | Success |
| 11 | CapabilityDelegate | R0=参数块 | Success |
| 12 | MemoryAlloc | R0=object_id, R1=size_hint | StateId → R0 |
| 13 | Abort | R0=reason_tag（0..4） | MachineStatus::Aborted |

非法 service_id / 畸形参数块 → `TrapReason::InvalidEncoding`（不 panic）。

### 身份与权限（语义不变）

- **TRAP 0（ObjectBirth）不切换** `current_object`；创建者身份保持不变。  
  新对象 self-AdminCap 会 attach 到当前事务 ctx，之后可用 **CALL** 进入。
- **TRAP 2（ObjectLink）不切换身份**；commit 时以调用者真实 `current_object` 做 capability 校验。
- 身份切换仅：**CALL** → 目标对象；**RETURN** → 调用者。

## 四、HostCall（非 Kernel service）

```
HOST_CALL <call_id>
```

宿主边界能力（Time/Random/Write/Read/Spawn 等），**不属于** KernelCall，不走 TRAP。  
未知 call_id → InvalidEncoding。见 `src/host.rs`。

---

## 编程约定

1. TRAP 0 后立即保存 ID：返回值在 R0；多次创建前用寄存器复制。  
2. TRAP 2 前必须把 from/to/link_type 放进 R0/R1/R2。  
3. 对象 ID 由内核分配，不可假设固定数值。  
4. 事务提交用 **TRAP 5**，不是旧 `COMMIT` 助记符。

## 端到端示例（TRAP）

```
module world_demo
version 1.0.0
    TRAP 0
    LOAD_CONST R4, 0
    ADD R1, R0, R4
    CALL R1, a_body
    TRAP 0
    LOAD_CONST R4, 0
    ADD R2, R0, R4
    ; … 布置 R0=from R1=to R2=1(owns)
    TRAP 2
    TRAP 5
    HALT
a_body:
    WRITE 0, "hello"
    RETURN
```

完整演示见仓库根目录 `world_demo.vasm`、`programs/`。
