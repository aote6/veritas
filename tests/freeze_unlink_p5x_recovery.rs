//! P5.x: Freeze / Unlink 在 WAL recovery 与 checkpoint 路径上的拓扑恢复等价性。
//!
//! 验证内容：freeze/unlink 后的对象状态与 link 拓扑在 crash-recovery 后与 live 一致。
//! 对应 VERIFICATION_MAP：freeze_unlink_p5x_recovery.rs
//! 若失败，意味着 ObjectFreeze/Unlink 的持久化或恢复丢失状态，破坏拓扑一致性不变量。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::{LinkType, ObjectState, ObjectType};

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

fn freeze(kernel: &Kernel, id: u64) {
    let mut tx = kernel.test_begin_in_object(id);
    kernel
        .handle(&mut tx, KernelCall::ObjectFreeze { object_id: id })
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn death(kernel: &Kernel, id: u64) {
    let mut tx = kernel.test_begin_in_object(id);
    kernel
        .handle(&mut tx, KernelCall::ObjectDeath { object_id: id })
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn link(kernel: &Kernel, from: u64, to: u64, lt: LinkType) {
    let mut tx = kernel.test_begin_in_object(from);
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantor: from,
                grantee: from,
                capability_type: "link".to_string(),
                resource: to,
            },
        )
        .unwrap();
    kernel
        .handle(
            &mut tx,
            KernelCall::ObjectLink {
                from,
                to,
                link_type: lt,
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn unlink(kernel: &Kernel, from: u64, to: u64) {
    let mut tx = kernel.test_begin_in_object(from);
    kernel
        .handle(&mut tx, KernelCall::ObjectUnlink { from, to })
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

/// P5.x: Freeze → Death sequence survives recovery
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn freeze_then_death_survives_recovery() {
    let wal_path = format!("target/test_freeze_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let target: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        target = birth(&kernel);
        freeze(&kernel, target);
        death(&kernel, target);
        assert!(
            kernel.test_engine().is_object_dead(target),
            "must be dead before crash"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert!(
            kernel.test_engine().is_object_dead(target),
            "must be dead after recovery"
        );
        assert_eq!(
            kernel.test_engine().get_object_state(target),
            Some(ObjectState::Dead)
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Link → Unlink sequence survives recovery
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn link_then_unlink_survives_recovery() {
    let wal_path = format!("target/test_link_unlink_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64;
    let b: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        a = birth(&kernel);
        b = birth_under(&kernel, a);
        link(&kernel, a, b, LinkType::Owns);
        unlink(&kernel, a, b);
        assert!(
            !kernel.test_engine().has_link(a, b),
            "link must be removed before crash"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(a),
            Some(ObjectState::Alive)
        );
        assert_eq!(
            kernel.test_engine().get_object_state(b),
            Some(ObjectState::Alive)
        );
        assert!(
            !kernel.test_engine().has_link(a, b),
            "link must not exist after recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Freeze + Unlink — freeze survives, unlink applied correctly after recovery
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn freeze_and_unlink_survives_recovery() {
    let wal_path = format!("target/test_freeze_unlink_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64;
    let b: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        a = birth(&kernel);
        b = birth_under(&kernel, a);
        link(&kernel, a, b, LinkType::Owns);
        freeze(&kernel, a);
        unlink(&kernel, a, b);
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(a),
            Some(ObjectState::Frozen),
            "a must be Frozen"
        );
        assert_eq!(
            kernel.test_engine().get_object_state(b),
            Some(ObjectState::Alive),
            "b must be Alive"
        );
        assert!(!kernel.test_engine().has_link(a, b), "link must be gone");
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Unlink + Death — unlinked target must survive owner death
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn unlink_then_death_target_survives() {
    let wal_path = format!("target/test_unlink_death_target_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64;
    let owned: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        owner = birth(&kernel);
        owned = birth_under(&kernel, owner);
        link(&kernel, owner, owned, LinkType::Owns);
        unlink(&kernel, owner, owned);
        death(&kernel, owner);
        assert!(
            kernel.test_engine().is_object_dead(owner),
            "owner must be dead"
        );
        assert!(
            !kernel.test_engine().is_object_dead(owned),
            "owned must survive: unlinked before death"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert!(
            kernel.test_engine().is_object_dead(owner),
            "owner dead after recovery"
        );
        assert!(
            !kernel.test_engine().is_object_dead(owned),
            "owned must survive recovery"
        );
        assert!(
            !kernel.test_engine().has_link(owner, owned),
            "link must be gone"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Death cascade — OWNS link, owner death cascades to owned
/// @category: C
/// @layer: recovery
/// @testworld: FORBIDDEN
/// @req: REC-06
#[test]
fn death_cascade_survives_recovery() {
    let wal_path = format!("target/test_death_cascade_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64;
    let owned: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        owner = birth(&kernel);
        owned = birth_under(&kernel, owner);
        link(&kernel, owner, owned, LinkType::Owns);
        death(&kernel, owner);
        assert!(
            kernel.test_engine().is_object_dead(owner),
            "owner must be dead"
        );
        assert!(
            kernel.test_engine().is_object_dead(owned),
            "owned must cascade to dead"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert!(
            kernel.test_engine().is_object_dead(owner),
            "owner dead after recovery"
        );
        assert!(
            kernel.test_engine().is_object_dead(owned),
            "owned dead after recovery"
        );
        assert!(
            !kernel.test_engine().has_link(owner, owned),
            "link must be gone"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
