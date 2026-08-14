//! WAL recovery 对 Object 生命周期（birth/death/link）的正确恢复。
//!
//! 验证内容：Object 相关操作经 WAL 重放后状态与拓扑一致。
//! 对应 VERIFICATION_MAP：wal_recovery_object.rs
//! 若失败，意味着 Object 持久化或恢复路径丢失生命周期信息。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;
use veritas_kernel::types::{LinkType, ObjectState};

fn birth_under(kernel: &Kernel, creator: u64) -> u64 {
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

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.test_begin();
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

/// P29.1: Object created and committed must survive WAL recovery.
/// This is the minimal replay test: birth → crash → recover → verify.
#[test]
fn object_birth_survives_recovery() {
    let wal_path = format!("target/test_obj_birth_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let object_id: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        object_id = birth(&kernel);
        assert_eq!(
            kernel.test_engine().get_object_state(object_id),
            Some(ObjectState::Alive),
            "object should be alive after commit"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(object_id),
            Some(ObjectState::Alive),
            "object must survive WAL recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.1: Object birth + link must both survive recovery.
/// Verifies topology is rebuilt correctly from WAL.
#[test]
fn object_link_survives_recovery() {
    let wal_path = format!("target/test_obj_link_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let obj_a: u64;
    let obj_b: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        obj_a = birth(&kernel);
        obj_b = birth_under(&kernel, obj_a);
        let mut tx = kernel.test_begin_in_object(obj_a);
        kernel
            .handle(
                &mut tx,
                KernelCall::CapabilityGrant {
                    grantor: obj_a,
                    grantee: obj_a,
                    capability_type: "link".to_string(),
                    resource: obj_b,
                },
            )
            .unwrap();
        kernel
            .handle(
                &mut tx,
                KernelCall::ObjectLink {
                    from: obj_a,
                    to: obj_b,
                    link_type: LinkType::Owns,
                },
            )
            .unwrap();
        kernel.handle(&mut tx, KernelCall::Commit).unwrap();
        assert!(
            kernel.test_engine().has_link(obj_a, obj_b),
            "link should exist before crash"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(obj_a),
            Some(ObjectState::Alive)
        );
        assert_eq!(
            kernel.test_engine().get_object_state(obj_b),
            Some(ObjectState::Alive)
        );
        assert!(
            kernel.test_engine().has_link(obj_a, obj_b),
            "link must survive WAL recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.1: Aborted object must NOT appear after recovery.
#[test]
fn aborted_object_not_recovered() {
    let wal_path = format!("target/test_abort_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let object_id: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let mut tx = kernel.test_begin();
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
        object_id = id;
        kernel
            .handle(
                &mut tx,
                KernelCall::Abort {
                    reason: veritas_kernel::types::AbortReason::WriteConflict,
                },
            )
            .unwrap();
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(object_id),
            None,
            "aborted object must not appear after recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
