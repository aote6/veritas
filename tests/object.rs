//! Object 生命周期测试入口（子模块 object/birth.rs, lifecycle.rs, memory.rs）。
//!
//! 验证内容：ObjectBirth、生命周期状态机、内存与状态一致性。
//! 对应 VERIFICATION_MAP：object.rs
//! 子模块测试覆盖对象创建到死亡的核心不变量。

mod common;

#[path = "object/birth.rs"]
mod birth;

#[path = "object/memory.rs"]
mod memory;

#[path = "object/lifecycle.rs"]
mod lifecycle;
