use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectType;
use veritas_kernel::kernel::KernelCall;

#[test]
fn diagnose_live_vs_recovery_components() {
    let wal_path = format!("target/test_diag_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // Live engine: 写入 WAL
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

    // Recovery engine: 从同一 WAL 恢复，不做新操作
    let k2 = Kernel::with_wal_path(wal_path);
    let recovery = k2.engine().debug_root_components();

    println!("LIVE      s={} o={} t={} c={} sc={}", live.0, live.1, live.2, live.3, live.4);
    println!("RECOVERY  s={} o={} t={} c={} sc={}", recovery.0, recovery.1, recovery.2, recovery.3, recovery.4);

    if live.0 != recovery.0 { println!("DIFF: state_store"); }
    if live.1 != recovery.1 { println!("DIFF: object_registry"); }
    if live.2 != recovery.2 { println!("DIFF: topology"); }
    if live.3 != recovery.3 { println!("DIFF: capability_graph"); }
    if live.4 != recovery.4 { println!("DIFF: scope_registry"); }
}
