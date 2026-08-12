//! 三个多对象事务回归测试：abort 一致性 / 跨 session capability 隔离 / WAL recovery + link 去重。

use std::sync::Arc;
use veritas_kernel::kernel::Kernel;
use veritas_kernel::world_api::WorldService;
use veritas_kernel::types::ObjectState;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

#[test]
fn test_a_multi_object_abort_leaves_no_partial_state() {
    let wal_path = temp_wal("test_a_abort");
    let kernel = Arc::new(Kernel::with_wal_path(wal_path));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"A-data".to_vec(), Some(a)).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_write(sid, 0, b"B-data".to_vec(), Some(b)).unwrap();
    world.tx_link(sid, a, b, "owns").unwrap();

    world.tx_abort(sid).expect("abort should succeed");

    assert!(world.get_object(a).is_none(), "A must not exist after abort");
    assert!(world.get_object(b).is_none(), "B must not exist after abort");

    let links = world.list_links();
    assert!(!links.iter().any(|l| l.from == a && l.to == b), "no A->B link should exist after abort");

    let ids: Vec<_> = world.list_objects().iter().map(|o| o.id).collect();
    assert!(!ids.contains(&a));
    assert!(!ids.contains(&b));
}

#[test]
fn test_b_cross_session_capability_isolation() {
    let wal_path = temp_wal("test_b_isolation");
    let kernel = Arc::new(Kernel::with_wal_path(wal_path));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid_a = world.tx_begin(None).unwrap();
    let obj = world.tx_create_object(sid_a).unwrap();
    world.tx_write(sid_a, 0, b"owner-data".to_vec(), Some(obj)).expect("Session A write failed");
    world.tx_commit(sid_a).expect("Session A commit failed");

    let sid_b = world.tx_begin(None).unwrap();
    let result = world.tx_write(sid_b, 0, b"forged".to_vec(), Some(obj));
    assert!(result.is_err(), "Session B must be denied");
    let _ = world.tx_abort(sid_b);
}

#[test]
fn test_c_wal_recovery_multi_object_link_no_duplication() {
    let wal_path = temp_wal("test_c_recovery");
    let kernel = Arc::new(Kernel::with_wal_path(wal_path.clone()));
    let world = WorldService::new(Arc::clone(&kernel));

    let sid = world.tx_begin(None).unwrap();
    let a = world.tx_create_object(sid).unwrap();
    let b = world.tx_create_object(sid).unwrap();
    world.tx_link(sid, a, b, "owns").unwrap();
    world.tx_commit(sid).unwrap();

    let links_before = world.list_links();
    let count_before = links_before.iter().filter(|l| l.from == a && l.to == b).count();
    assert_eq!(count_before, 1, "exactly one A->B link before restart");

    drop(world);
    drop(kernel);

    let kernel2 = Arc::new(Kernel::with_wal_path(wal_path));
    let world2 = WorldService::new(Arc::clone(&kernel2));

    let obj_a = world2.get_object(a).expect("A must survive recovery");
    assert_eq!(obj_a.state, ObjectState::Alive);
    let obj_b = world2.get_object(b).expect("B must survive recovery");
    assert_eq!(obj_b.state, ObjectState::Alive);

    let links_after = world2.list_links();
    let count_after = links_after.iter().filter(|l| l.from == a && l.to == b).count();
    assert_eq!(count_after, 1, "exactly one A->B link after recovery");
    assert_eq!(count_after, count_before);
}
