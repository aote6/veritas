# Veritas 指令集

最后更新: 2026-08-10
状态: 31条指令全部可汇编、可执行、可持久化

## 操作数 (Operand)

指令操作数可以是立即数或寄存器:

立即数: 十进制(10)或十六进制(0x1A)
寄存器: R0-R7

设计原则: 任何可能由前序指令在运行时产生的值，必须使用 Operand 而非裸数值类型。

## 寄存器

8个通用寄存器 R0-R7。R0 同时是内核调用返回值寄存器。
标志位: zero, negative, overflow, carry。

## 一、计算与控制流 (12条)

NOP             空操作
LOAD_CONST Rn, imm    加载立即数到寄存器
ADD Rd, Rs1, Rs2      加法 Rd = Rs1 + Rs2
SUB Rd, Rs1, Rs2      减法 Rd = Rs1 - Rs2
CMP Rs1, Rs2          比较，设置 zero/negative 标志
JMP target            无条件跳转
JZ target             zero=1 时跳转
JNZ target            zero=0 时跳转
JN target             negative=1 时跳转
CALL object_id, pc    跨对象调用，切换 current_object
RETURN                从 CALL 返回
HALT                  停机

## 二、状态读写 (5条)

READ state_id             读取状态字节到 R0
WRITE state_id, "string"  写入字符串到状态
LOAD_STATE_U64 Rn, sid    加载状态中的 u64 到寄存器
LOAD_STATE_BYTES Rn, sid  加载状态字节到寄存器
WRITE_REGISTER sid, Rn    把寄存器值写入状态 (state_id仅立即数)

注意: WRITE 的 state_id 支持寄存器操作数，WRITE_REGISTER 保留作为历史兼容。

## 三、对象世界操作 (9条)

OBJECT_BIRTH 0            创建对象，ID返回到R0，不切换current_object(创建者身份保持不变)
OBJECT_DEATH object_id    销毁对象
OBJECT_FREEZE object_id   冻结对象(不可逆)
OBJECT_LINK from,to,type  建立链接，不切换身份；commit时以调用者真实current_object对from/to做capability校验
OBJECT_UNLINK from,to     断开链接
CAPABILITY_GRANT h,"p",r  授予权限
EFFECT "payload"          产生副作用事件
TRAP service_id           系统陷入(R0-R2传参)
HOST_CALL call_id         宿主调用

LinkType: owns / depends_on / references

身份切换:
  CALL    -> 目标对象 (须先通过 authorize_intent(AccessIntent::Call) 审计)
  RETURN  -> 调用者

OBJECT_BIRTH 不切换身份: 创建者持有新对象 AdminCap，但仍以创建者身份继续执行。
若需以新对象身份操作，必须显式 CALL 进入。

OBJECT_LINK 不切换身份: commit 时 authorize_intent(AccessIntent::Link(from,to))
以调用者真实 current_object 做权限校验(2026-08-10 修复，历史上曾用
enter_object(from) 自我授权绕过，已删除，回归测试见
tests/machine_object_link_security.rs)。

## 四、事务控制 (4条)

SAVEPOINT "name"    设置保存点
ROLLBACK_TO "name"  回滚到保存点
COMMIT              提交事务，生成TransactionDelta，写入WAL
ABORT               中止事务

约束: Transaction不可嵌套。CALL/RETURN不改变Transaction边界。
COMMIT只能在最外层执行。

## 编程约定

1. OBJECT_BIRTH后立即保存ID: 返回值在R0，如需多次创建，用 LOAD_CONST Rtmp,0 + ADD Rdst,R0,Rtmp 复制到其他寄存器
2. WRITE 替代 WRITE_REGISTER: WRITE的state_id支持寄存器操作数
3. 对象ID不可预测: 内核分配的实际ID取决于WAL恢复状态和事务顺序
4. 程序第一条指令建议为 OBJECT_BIRTH: 同时完成创建首个对象和建立执行身份

## 端到端验证程序

module world_demo
version 1.0.0
    OBJECT_BIRTH 0
    WRITE R0, "hello from veritas"
    LOAD_CONST R2, 0
    ADD R1, R0, R2
    OBJECT_BIRTH 0
    OBJECT_LINK R1, R0, owns
    COMMIT
    HALT

验证链路: VASM -> Assembler -> VMOD -> Codec -> Machine -> Operand Resolution
-> KernelCall -> TransactionContext -> commit -> TransactionDelta -> WAL -> Recovery -> World

首次端到端验证: 2026-08-10

## ISA 设计原则 (2026-08-10)

1. 操作数必须是 Operand
   任何可能由前序指令在运行时产生的值(ObjectId, StateId, CapabilityId,
   ScopeId, EffectId, Handle 等)，不得在 Instruction 中固化为裸数值类型。
   必须使用 Operand 或等价的动态寻址抽象。

2. 身份切换必须显式
   只有 OBJECT_BIRTH(自举)、CALL、RETURN 可以改变 current_object。
   业务操作(WRITE, LINK, FREEZE, DEATH等)不得隐式切换执行身份。

3. 定义层和入口层必须同步
   Instruction enum 新增指令时，必须同步更新:
   - assembler.rs (汇编解析)
   - machine.rs (执行分发)
   - instruction_codec.rs (二进制编解码)
   - 本文档 (指令说明)
