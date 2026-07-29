# Veritas Abstract Machine

Veritas 是一台抽象机器(Abstract Machine)，不是数据库，不是框架，不是内核。

它运行在现有 CPU 和操作系统之上，不需要修改硬件。
它为软件提供一层新的确定性运行时，就像 JVM 为 Java 提供运行时一样。

应用程序 / DSL / AI Agent / 模块
--------------------------
    Veritas Abstract Machine
--------------------------
    Rust
--------------------------
    Linux / Android / Windows
--------------------------
    CPU (x86 / ARM)

CPU 不知道什么叫事务、Capability、Contract。
这些都是 Veritas 抽象机器定义的新指令。


指令集实现进度
==============

事务类指令
----------
BEGIN           开始事务，记录快照版本                        已实现
READ            读取状态，自动追踪到读取集                    已实现
WRITE           写入暂存区，不立即生效                        已实现
COMMIT          冲突检测-写WAL-固化-执行副作用                已实现
ABORT           丢弃暂存修改和副作用                          已实现

回滚类指令
----------
SAVEPOINT       创建命名回滚点(含scope变更)                   已实现
ROLLBACK_TO     回滚到指定保存点                              已实现

副作用指令
----------
EFFECT          暂存副作用，提交后执行，携带幂等键            已实现

集合类指令
----------
ENUM_SCOPE          枚举动态集合，记录结构版本(幻读防御)      已实现
BIND_TO_SCOPE       绑定状态到集合                            已实现
UNBIND_FROM_SCOPE   从集合解绑                                已实现

能力类指令
----------
GRANT           授予能力凭证(确定性哈希生成)                  纯数据层完成
REVOKE          回收凭证(级联/非级联)                         纯数据层完成
DELEGATE        委派能力(森林约束防环)                        纯数据层完成
HOLD_CHECK      检查是否持有能力                              未实现

契约类指令
----------
REQUIRE         前置条件检查                                  未实现
ENSURE          后置条件检查                                  未实现
INVARIANT       局部不变量检查                                未实现

模块类指令
----------
LOAD            加载模块                                      未实现
LINK            组合模块，静态契约检查                        未实现
UNLOAD          卸载模块                                      未实现


机器内部机制(不暴露为指令)
==========================
WAL 写入与 fsync                                             已实现
崩溃恢复(重放WAL、重建Scope、重试Effect、tx_id续接)          已实现
冲突检测(读写反依赖 + 幻读)                                  已实现
快照隔离(ReadFutureVersion检测)                              已实现
提交临界区(串行化)                                           已实现


测试: 48 个，全部通过
最后更新: 2026-07-29
