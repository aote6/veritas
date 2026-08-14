//! WAL recovery 过程中的安全与一致性不变量。
//!
//! 验证内容：recovery 后对象状态、link、capability 等满足与 live 相同的不变量。
//! 对应 VERIFICATION_MAP：wal_recovery_invariants.rs
//! 若失败，意味着 recovery 引入非法状态或破坏既有安全不变量。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;
use veritas_kernel::types::{LinkType, ObjectState};

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

fn death(kernel: &Kernel, id: u64) {
    let mut tx = kernel.test_begin_in_object(id);
    kernel
        .handle(&mut tx, KernelCall::ObjectDeath { object_id: id })
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn freeze(kernel: &Kernel, id: u64) {
    let mut tx = kernel.test_begin_in_object(id);
    kernel
        .handle(&mut tx, KernelCall::ObjectFreeze { object_id: id })
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

/// P29.2: Birth → Death sequence must be correctly replayed.
/// After recovery, object must be Dead (not Alive, not missing).
#[test]
fn recovery_invariant_birth_then_death() {
    let wal_path = format!("target/test_inv_birth_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let obj = birth(&kernel);
        death(&kernel, obj);
        assert!(
            kernel.test_engine().is_object_dead(obj),
            "object must be dead before crash"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let obj = kernel
            .test_engine()
            .list_object_ids()
            .into_iter()
            .find(|id| *id != 0)
            .unwrap_or(1);
        assert!(
            kernel.test_engine().is_object_dead(obj),
            "object must be dead after recovery"
        );
        assert_ne!(
            kernel.test_engine().get_object_state(obj),
            Some(ObjectState::Alive)
        );
        assert_ne!(
            kernel.test_engine().get_object_state(obj),
            Some(ObjectState::Frozen)
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Freeze → Death sequence.
/// After recovery, final state must be Dead (not Frozen).
#[test]
fn recovery_invariant_birth_freeze_then_death() {
    let wal_path = format!(
        "target/test_inv_birth_freeze_death_{}.wal",
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal_path);

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let obj = birth(&kernel);
        freeze(&kernel, obj);
        death(&kernel, obj);
        assert!(
            kernel.test_engine().is_object_dead(obj),
            "must be dead before crash"
        );
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        let obj = kernel
            .test_engine()
            .list_object_ids()
            .into_iter()
            .find(|id| *id != 0)
            .unwrap_or(1);
        assert!(
            kernel.test_engine().is_object_dead(obj),
            "must be dead after recovery"
        );
        assert_eq!(
            kernel.test_engine().get_object_state(obj),
            Some(ObjectState::Dead),
            "final state must be Dead, not Frozen"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P29.2: Birth → Link → Unlink.
/// After recovery, link must not exist, objects must be alive.
#[test]
fn recovery_invariant_link_then_unlink() {
    let wal_path = format!("target/test_inv_link_unlink_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64;
    let b: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        a = birth(&kernel);
        b = birth(&kernel);
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

/// P29.2: Birth → Link → Death of owner.
/// After recovery: owner Dead, link gone, no dangling edges.
#[test]
fn recovery_invariant_owner_death_removes_link() {
    let wal_path = format!("target/test_inv_owner_death_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64;
    let owned: u64;

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        owner = birth(&kernel);
        owned = birth(&kernel);
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
        assert!(
            !kernel.test_engine().has_link(owner, owned),
            "link must be removed"
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
            "no dangling link after recovery"
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
