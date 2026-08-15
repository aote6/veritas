//! world_demo: 端到端真实执行 demo。
//! birth A -> write A -> birth B -> write B -> link A->B -> 单次 commit -> WAL recovery -> 校验一致
//! 通过 WorldService session 完成，同一 session/同一 TransactionContext 内一次性 commit。

use std::sync::Arc;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectState;
use veritas_kernel::world_api::WorldService;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

/// 多对象 birth/write/link/commit/recover 端到端演示路径正确。
/// 失败意味着核心世界演化路径在 recovery 后不一致。
/// @category: D
/// @layer: integration
/// @testworld: NOT_USED
/// @req: INT-02
#[test]
fn world_demo_multi_object_birth_write_link_commit_recover() {
    let wal_path = temp_wal("world_demo");

    let kernel = Arc::new(Kernel::with_wal_path(wal_path.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).expect("tx_begin failed");

    let a = world.tx_create_object(sid).expect("birth A failed");
    world
        .tx_write(sid, 0, b"hello-A".to_vec(), Some(a))
        .expect("write A failed");

    let b = world.tx_create_object(sid).expect("birth B failed");
    // link before write B so current_object is still A (birth doesn't switch)
    world.tx_link(sid, a, b, "owns").expect("link A->B failed");
    world
        .tx_write(sid, 0, b"hello-B".to_vec(), Some(b))
        .expect("write B failed");

    let receipt = world.tx_commit(sid).expect("single commit failed");
    println!(
        "demo commit: tx_id={} before_root={} after_root={}",
        receipt.tx_id, receipt.before_root, receipt.after_root
    );

    let obj_a = world.get_object(a).expect("A should exist pre-restart");
    assert_eq!(obj_a.state, ObjectState::Alive);
    let obj_b = world.get_object(b).expect("B should exist pre-restart");
    assert_eq!(obj_b.state, ObjectState::Alive);
    let links = world.list_links();
    assert!(
        links.iter().any(|l| l.from == a && l.to == b),
        "A->B link should exist pre-restart"
    );

    drop(world);
    drop(kernel);

    let kernel2 = Arc::new(Kernel::with_wal_path(wal_path));
    let world2 = WorldService::new(Arc::clone(&kernel2));

    let obj_a2 = world2.get_object(a).expect("A should survive restart");
    assert_eq!(
        obj_a2.state,
        ObjectState::Alive,
        "A must remain Alive after recovery"
    );
    let obj_b2 = world2.get_object(b).expect("B should survive restart");
    assert_eq!(
        obj_b2.state,
        ObjectState::Alive,
        "B must remain Alive after recovery"
    );

    let links2 = world2.list_links();
    assert!(
        links2.iter().any(|l| l.from == a && l.to == b),
        "A->B link should survive restart"
    );

    println!("world_demo PASS: A={} B={} link survived recovery", a, b);
}
