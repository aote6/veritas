content = '''# Veritas Module Specification v0.1

## 1. Module 是什么

Module 是 Veritas 中的可执行单元。它是一个携带代码、权限声明
和验证规则的 Object。

Module 不是文件。不是 Rust trait。Module 是 Veritas Object 的
一个子类型。

## 2. Module 的结构

Module 是 ObjectType = ModuleObject 的 Object。它包含:

- 标准 Object 的所有字段 (id, type, memory_space, capability_space 等)
- code_section: 指令序列
- import_section: 依赖的其他 Module
- export_section: 对外暴露的入口点
- permission_section: 需要的 Capability 声明
- verification_rule: 执行前必须满足的验证规则

## 3. Module 的格式

当前格式: .vmod 二进制格式

未来应定义 Module 作为 Object 的序列化格式，使其可以直接存储在
Memory Space 中。

## 4. Module 的加载

加载 Module 的步骤:
1. 验证 Module 的完整性
2. 创建一个新的 ModuleObject
3. 将 code_section 写入该 Object 的 Memory Space
4. 根据 permission_section 请求 Capability
5. 将 Module 的 import 解析为对其他 Object 的 Capability
6. 将 Module 的 export 注册到 Capability 空间
7. Module 进入 ACTIVE 状态

## 5. Module 的执行

- 执行 Module 需要持有 ExecuteCap
- Machine 从 Module 的 code_section 读取指令
- PC 在 Module 的代码空间内移动
- 跨 Module 调用通过 CALL 指令 + Capability 实现

## 6. Module 之间的调用

- Module A 调用 Module B 需要持有 B 的 ExecuteCap
- 调用通过 CALL 指令 + CapabilityId 实现
- Machine 切换执行上下文到 B 的代码空间
- B 执行完毕后通过 RETURN 指令返回 A

## 7. Module 的验证规则

Module 可以携带验证规则。验证规则是 Module Object 的属性，
在加载时和执行前由 Machine 检查。

验证规则类型:
- 允许访问的 ObjectId 白名单
- 允许的指令类型限制
- 允许的 Capability 操作限制
- 执行次数限制

## 8. Module 与 Object 的关系

- Module 本身是一个 Object
- Module 可以持有 Capability (访问其他 Object)
- Module 可以被其他 Object 通过 Capability 引用
- Module 可以拥有 State (通过其 Memory Space)
- Module 死亡时，其代码和状态一起回收

## 9. 当前实现映射

| 规范定义 | 当前代码 | 未来方向 |
|---|---|---|
| ModuleObject | module.rs ModuleImage | 统一为 Object 子类型 |
| .vmod 格式 | module.rs | 保持或扩展 |
| 加载器 | module.rs ModuleLoader | 转为内核服务 |
| 执行 | machine.rs boot/step | 保持 |
| 验证规则 | verifier.rs | 集成到 Object 属性 |

## 10. 实现要求

1. Module 必须是 Object
2. Module 的代码存储在 Memory Space 中
3. 跨 Module 调用通过 Capability + CALL 指令
4. 验证规则是 Module 的属性，Machine 自动检查
'''

with open('docs/constitution/module.md', 'w') as f:
    f.write(content)
print('Done: docs/constitution/module.md')
