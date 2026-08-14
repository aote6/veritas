//! Transaction 测试入口（子模块 transaction/ 等）。
//!
//! 验证内容：事务 begin/commit/abort 语义与原子性。
//! 对应 VERIFICATION_MAP：transaction.rs
//! 子模块覆盖 commit 路径核心不变量。

mod common;

#[path = "transaction/commit.rs"]
mod commit;

#[path = "transaction/isolation.rs"]
mod isolation;

#[path = "transaction/conflict.rs"]
mod conflict;
