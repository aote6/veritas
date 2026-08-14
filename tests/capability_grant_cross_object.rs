//! P0 回归：CapabilityGrant 的 grantor 语义必须真实生效。
//!
//! 验证内容：
//! - 授权记录中的 granted_by 必须是真实发起授权的对象（A），而非 grantee 自授。
//! - 未持有 capability 时跨对象 ObjectLink 在 commit 被拒绝。
//! - 持有正确 grant 后，被授权者可成功 link 目标资源。
//!
//! 对应 VERIFICATION_MAP：capability_grant_cross_object.rs / grantor_is_real_authorizer_not_self_grant
//!
//! 若本测试失败，意味着 CapabilityGrant 的授权者身份可被伪造或被 grantee 冒充，
//! 破坏“授权可归因、不可自授”的安全不变量。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{LinkType, ObjectType};

fn birth(kernel: &Kernel) -> u64 {
    let mut ctx = kernel.test_begin();
    let id = match kernel
        .handle(&mut ctx, KernelCall::ObjectBirth { object_type: ObjectType::StateObject })
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut ctx, KernelCall::Commit).unwrap();
    id
}

/// 验证 grantor 是真实授权者而非自授：A 授权 B 对 C 的 link 能力后，
/// capability 记录中 granted_by=A、holder=B，且 B 随后可成功 link C。
/// 失败意味着授权归因可被伪造，破坏 CapabilityGrant 的安全不变量。
#[test]
fn grantor_is_real_authorizer_not_self_grant() {
    let kernel = Kernel::new();

    // A 授权者；B 被授权者；C 资源（在 A 下 birth，使 A 持有 AdminCap(C)）
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = {
        let mut tx = kernel.test_begin_in_object(a);
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
    };

    // 未授权前：B 尝试 link 到 C 应当在 commit 时失败
    {
        let mut tx = kernel.test_begin_in_object(b);
        let _ = kernel.handle(&mut tx, KernelCall::ObjectLink { from: b, to: c, link_type: LinkType::Owns });
        let commit_result = kernel.handle(&mut tx, KernelCall::Commit);
        assert!(commit_result.is_err(), "未持有 capability 时，B 对 C 的 link 必须在 commit 时被拒绝");
    }

    // A 以自己的身份（current_object = a）把 link 能力授予 B，作用在资源 C 上
    let cap_id = {
        let mut tx = kernel.test_begin_in_object(a);
        let cap_id = match kernel
            .handle(
                &mut tx,
                KernelCall::CapabilityGrant {
                    grantor: a,
                    grantee: b,
                    capability_type: "link".to_string(),
                    resource: c,
                },
            )
            .unwrap()
        {
            TrapResult::CapabilityId(id) => id,
            _ => panic!("expected CapabilityId"),
        };
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
        cap_id
    };

    // 核心断言：grant 记录里 granted_by 是 A，holder 是 B，且 A != B —— 不是自授
    let records = kernel.test_capability_records();
    let record = records
        .into_iter()
        .find(|r| r.capability_id == cap_id)
        .expect("刚授予的 capability 必须能在 test_capability_records 中查到");
    assert_eq!(record.granted_by, a, "granted_by 必须是真实授权者 A，不能被 grantee 冒充");
    assert_eq!(record.holder, b, "holder 必须是被授权者 B");
    assert_ne!(record.granted_by, record.holder, "grantor != grantee，杜绝自授语义");

    // 拿到授权后：B 用该 capability 对 C 执行 link 应当成功
    let mut tx = kernel.test_begin_in_object(b);
    let link_result = kernel.handle(&mut tx, KernelCall::ObjectLink { from: b, to: c, link_type: LinkType::Owns });
    assert!(link_result.is_ok(), "B 持有 A 授予的 capability 后，对 C 的 link 应当成功");
    let commit_result = kernel.handle(&mut tx, KernelCall::Commit);
    assert!(commit_result.is_ok(), "commit 应当成功：B 持有对 C 的有效授权");
    assert!(kernel.test_engine().has_link(b, c), "link 关系必须真实建立");
}
