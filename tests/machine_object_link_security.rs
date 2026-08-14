//! P0 安全回归测试：防止 OBJECT_LINK 身份伪造漏洞重现。
//!
//! 历史 bug：machine.rs 的 ObjectLink 分支曾经在调用 kernel 之前
//! 执行 `self.ctx.enter_object(from)`，导致 commit 时的
//! authorize_intent(AccessIntent::Link(from, to)) 检查里
//! `target == ctx.current_object` 恒真，从而让调用者无需持有
//! 任何 capability 就能伪造任意两个对象之间的 Link。
//!
//! 本文件锁定两件事：
//! 1. 恶意路径：调用者对 from/to 均无 capability 时，Link 必须被拒绝。
//! 2. 合法路径：调用者持有正确 capability 时，Link 仍然必须成功
//!    （证明 P0 修复没有误伤正常授权流程）。

mod common;

use common::new_kernel;
use veritas_kernel::kernel::{KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{LinkType, ObjectType};

fn birth_under(kernel: &veritas_kernel::kernel::Kernel, creator: u64) -> u64 {
    let mut tx = kernel.test_begin_in_object(creator);
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

/// 恶意路径：X 对 A 有 AdminCap（是 A 的创建者），
/// 但对 B 没有任何 capability。
/// X 尝试以 A 的身份 Link(A -> B)，必须在 commit 时被拒绝。
#[test]
fn object_link_without_capability_on_target_is_rejected() {
    let tk = new_kernel();

    // X 创建 A：X 获得 A 的 AdminCap
    let x = tk.root_object;
    let a = birth_under(&tk.kernel, x);

    // B 由完全独立的第三方创建，X 对 B 没有任何权限关系
    let stranger = birth_under(&tk.kernel, x);
    let b = birth_under(&tk.kernel, stranger);

    // X 以 A 的身份尝试 Link(A -> B)，中途不做任何 CapabilityGrant
    let mut tx = tk.kernel.test_begin_in_object(a);
    let handle_result = tk.kernel.handle(
        &mut tx,
        KernelCall::ObjectLink {
            from: a,
            to: b,
            link_type: LinkType::Owns,
        },
    );
    // ObjectLink 本身只是 push 到 pending_links，预期这一步不报错
    assert!(
        handle_result.is_ok(),
        "object_link staging should not itself error"
    );

    // 真正的授权检查发生在 commit
    let commit_result = tk.kernel.handle(&mut tx, KernelCall::Commit);
    assert!(
        commit_result.is_err(),
        "commit must reject Link(A,B) when caller holds no capability on B \
         (regression test for the enter_object(from) self-authorization bypass)"
    );

    // 双重确认：topology 中确实没有出现这条边
    assert!(
        !tk.kernel.has_link(a, b),
        "no link edge should exist after a rejected commit"
    );
}

/// 合法路径：X 对 A 和 B 都持有 capability 时，Link 必须成功。
/// 用于证明去掉 enter_object(from) 之后，正常授权流程没有被误伤。
#[test]
fn object_link_with_proper_capability_succeeds() {
    let tk = new_kernel();

    let x = tk.root_object;
    let a = birth_under(&tk.kernel, x);
    let b = birth_under(&tk.kernel, a);

    // X 以 A 的身份，先为自己在 B 上授予 link 权限，再执行 Link
    let mut tx = tk.kernel.test_begin_in_object(a);
    tk.kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantor: a,
                grantee: a,
                capability_type: "link".to_string(),
                resource: b,
            },
        )
        .unwrap();
    tk.kernel
        .handle(
            &mut tx,
            KernelCall::ObjectLink {
                from: a,
                to: b,
                link_type: LinkType::Owns,
            },
        )
        .unwrap();

    let commit_result = tk.kernel.handle(&mut tx, KernelCall::Commit);
    assert!(
        commit_result.is_ok(),
        "commit should succeed when caller holds capability on target"
    );

    assert!(
        tk.kernel.has_link(a, b),
        "link edge should exist after a successful commit"
    );
}
