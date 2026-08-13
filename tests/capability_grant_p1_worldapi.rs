// P1: veritasd 外部接口暴露 tx_capability_grant —— WorldService 层集成测试。
// 验证：A grant B link capability on C → commit → B 新 session 对 C 的 link 成功；
// 未授权的 B 操作在 commit 时仍然失败；grantor 语义保持真实（A != B）。
use std::sync::Arc;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::world_api::WorldService;

#[test]
fn tx_capability_grant_external_interface_end_to_end() {
    let kernel = Arc::new(Kernel::new());
    let world = WorldService::new(Arc::clone(&kernel));

    // Bootstrap identity A.
    let a = world.attach_identity(None).unwrap();

    // A creates B and C within one session, then commits so both are Alive.
    let sid0 = world.tx_begin(Some(a)).unwrap();
    let b = world.tx_create_object(sid0).unwrap();
    let c = world.tx_create_object(sid0).unwrap();
    world.tx_commit(sid0).unwrap();

    // Unauthorized: B has no capability on C yet — tx_link stages fine,
    // but commit must reject it (authorization is enforced at commit time).
    {
        let sid = world.tx_begin(Some(b)).unwrap();
        world.tx_link(sid, b, c, "owns").unwrap();
        let commit_result = world.tx_commit(sid);
        assert!(
            commit_result.is_err(),
            "B 未持有 capability 时，对 C 的 link 必须在 commit 时被拒绝"
        );
    }

    // A grants B a "link" capability on C via the new external primitive.
    let sid1 = world.tx_begin(Some(a)).unwrap();
    world
        .tx_capability_grant(sid1, a, b, "link".to_string(), c)
        .unwrap();
    world.tx_commit(sid1).unwrap();

    // Authorized: B's new session can now link to C.
    let sid2 = world.tx_begin(Some(b)).unwrap();
    world.tx_link(sid2, b, c, "owns").unwrap();
    let receipt = world.tx_commit(sid2).unwrap();
    assert_ne!(receipt.before_root, receipt.after_root, "commit 必须改变 root hash");
    assert!(kernel.has_link(b, c), "link 关系必须真实建立");

    // grantor authenticity: query the live capability graph via Engine test helper.
    let records = kernel.test_capability_records();
    let record = records
        .into_iter()
        .find(|r| r.holder == b && r.resource == c)
        .expect("刚授予的 capability 必须能在 capability graph 中查到");
    assert_eq!(record.granted_by, a, "granted_by 必须是真实授权者 A");
    assert_eq!(record.holder, b, "holder 必须是被授权者 B");
    assert_ne!(record.granted_by, record.holder, "grantor != grantee，杜绝自授语义");
}
