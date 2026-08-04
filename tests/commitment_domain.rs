use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectType;
use veritas_kernel::kernel::KernelCall;

#[test]
fn diagnose_live_vs_recovery_components() {
    let wal_path = format!("target/test_diag_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k1 = Kernel::with_wal_path(wal_path.clone());
    let root = {
        let mut tx = k1.begin();
        let result = k1.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k1.commit(&mut tx).unwrap();
        id
    };
    let mut tx = k1.begin_in_object(root);
    k1.write(&mut tx, 0, vec![99]).unwrap();
    k1.commit(&mut tx).unwrap();

    let live = k1.engine().debug_root_components();
    let k2 = Kernel::with_wal_path(wal_path);
    let recovery = k2.engine().debug_root_components();

    println!("LIVE      s={} o={} t={} c={} sc={}", live.0, live.1, live.2, live.3, live.4);
    println!("RECOVERY  s={} o={} t={} c={} sc={}", recovery.0, recovery.1, recovery.2, recovery.3, recovery.4);

    assert_eq!(live, recovery, "All five components must match between live and recovery");
}

#[test]
fn self_access_does_not_grow_capability_graph() {
    let wal_path = format!("target/test_nogrow_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path);
    let root = {
        let mut tx = k.begin();
        let result = k.handle(&mut tx, KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        }).unwrap();
        let id = match result {
            veritas_kernel::kernel::TrapResult::ObjectId(id) => id,
            _ => panic!(),
        };
        k.commit(&mut tx).unwrap();
        id
    };

    let before = k.engine().capability_sequence();

    for i in 0..5 {
        let mut tx = k.begin_in_object(root);
        k.write(&mut tx, 0, vec![i as u8]).unwrap();
        k.commit(&mut tx).unwrap();
    }

    let after = k.engine().capability_sequence();
    assert_eq!(before, after,
        "self-access must not create CapabilityGraph records");
}
