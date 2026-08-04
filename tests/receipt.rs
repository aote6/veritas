use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectType;
use veritas_kernel::kernel::KernelCall;

#[test]
fn receipt_after_matches_root_hash() {
    let wal_path = format!("target/test_rcpt1_{}.wal", std::process::id());
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

    let mut tx = k.begin_in_object(root);
    k.write(&mut tx, 0, vec![42]).unwrap();
    let receipt = k.commit(&mut tx).unwrap();

    assert_eq!(receipt.after_root, k.engine().root_hash());
}

#[test]
fn receipt_before_after_consistency() {
    let wal_path = format!("target/test_rcpt2_{}.wal", std::process::id());
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

    let mut tx = k.begin_in_object(root);
    k.write(&mut tx, 0, vec![42]).unwrap();
    let receipt = k.commit(&mut tx).unwrap();

    assert_ne!(receipt.before_root, 0);
    assert_ne!(receipt.after_root, 0);
    assert_ne!(receipt.before_root, receipt.after_root);
    assert_eq!(receipt.after_root, k.engine().root_hash());
}

#[test]
fn receipt_replay_consistency() {
    let wal_path = format!("target/test_rcpt3_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let k = Kernel::with_wal_path(wal_path.clone());
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
    let mut tx = k.begin_in_object(root);
    k.write(&mut tx, 0, vec![99]).unwrap();
    let receipt = k.commit(&mut tx).unwrap();

    // Replay 和 idle Recovery 必须一致
    let recovery_hash = {
        let k2 = Kernel::with_wal_path(wal_path.clone());
        k2.engine().root_hash()
    };
    let replay_hash = Kernel::replay(&wal_path);

    assert_eq!(replay_hash, recovery_hash,
        "Replay must equal idle Recovery root_hash");
    assert_eq!(receipt.after_root, k.engine().root_hash(),
        "Receipt.after_root must equal live engine root_hash");
}
