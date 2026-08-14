//! Machine 层基础测试入口（子模块 machine/basic.rs）。
//!
//! 验证内容：Machine 执行、指令调度、基础运行时行为。
//! 对应 VERIFICATION_MAP：machine.rs
//! 子模块测试覆盖指令级正确性；本文件仅作模块组织。

mod common;

#[path = "machine/basic.rs"]
mod basic;
