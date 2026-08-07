//! P4: CapabilityDelegate WAL Closure — topology recovery equivalence.

use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::ObjectType;

fn temp_wal(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("veritas_p4_{}_{}.wal", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
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

fn grant(kernel: &Kernel, grantee: u64, resource: u64) -> u64 {
    let mut tx = kernel.test_begin();
    let cap_id = match kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityGrant {
                grantee,
                capability_type: "read".into(),
                resource,
            },
        )
        .unwrap()
    {
        TrapResult::CapabilityId(id) => id,
        _ => panic!("expected CapabilityId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    cap_id
}

fn delegate(kernel: &Kernel, cap: u64, from: u64, to: u64, cascade: bool) {
    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityDelegate {
                capability_id: cap,
                from,
                to,
                cascade_on_revoke: cascade,
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn revoke(kernel: &Kernel, cap: u64, holder: u64, cascade_override: Option<bool>) {
    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityRevoke {
                capability_id: cap,
                holder,
                cascade_override,
            },
        )
        .unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

/// T1: grant → delegate → commit → checkpoint restore preserves topology.
#[test]
fn t1_delegate_survives_checkpoint() {
    let wal = temp_wal("t1");
    let kernel = Kernel::with_wal_path(wal.clone());
    let a = birth(&kernel);
    let b = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);
    delegate(&kernel, cap, a, b, true);

    assert!(kernel.test_engine().holds_capability(cap, a));
    assert!(kernel.test_engine().holds_capability(cap, b));

    let snap = kernel.create_checkpoint();
    let wal2 = temp_wal("t1b");
    let k2 = Kernel::with_wal_path(wal2);
    assert!(k2.restore_checkpoint(&snap));
    assert!(k2.test_engine().holds_capability(cap, a));
    assert!(k2.test_engine().holds_capability(cap, b));
}

/// T2: multi-level A→B→C→D tree.
#[test]
fn t2_multilevel_delegate_tree() {
    let kernel = Kernel::with_wal_path(temp_wal("t2"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    let d = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);
    delegate(&kernel, cap, a, b, true);
    delegate(&kernel, cap, b, c, true);
    delegate(&kernel, cap, c, d, false);

    for h in [a, b, c, d] {
        assert!(kernel.test_engine().holds_capability(cap, h), "holder {}", h);
    }

    let snap = kernel.create_checkpoint();
    let records: Vec<_> = snap
        .capability_records
        .iter()
        .filter(|r| r.capability_id == cap)
        .cloned()
        .collect();
    assert_eq!(records.len(), 4);
    let parent_of = |holder: u64| {
        records
            .iter()
            .find(|r| r.holder == holder)
            .and_then(|r| r.parent)
    };
    assert_eq!(parent_of(a), None);
    assert_eq!(parent_of(b), Some(a));
    assert_eq!(parent_of(c), Some(b));
    assert_eq!(parent_of(d), Some(c));
}

/// T3: cascade revoke deactivates subtree.
#[test]
fn t3_cascade_revoke() {
    let kernel = Kernel::with_wal_path(temp_wal("t3"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);
    delegate(&kernel, cap, a, b, true);
    delegate(&kernel, cap, b, c, true);
    revoke(&kernel, cap, b, Some(true));

    assert!(kernel.test_engine().holds_capability(cap, a));
    assert!(!kernel.test_engine().holds_capability(cap, b));
    assert!(!kernel.test_engine().holds_capability(cap, c));
}

/// T4: non-cascade revoke preserves downstream.
#[test]
fn t4_non_cascade_revoke() {
    let kernel = Kernel::with_wal_path(temp_wal("t4"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);
    delegate(&kernel, cap, a, b, false);
    delegate(&kernel, cap, b, c, true);
    revoke(&kernel, cap, b, None);

    assert!(kernel.test_engine().holds_capability(cap, a));
    assert!(!kernel.test_engine().holds_capability(cap, b));
    assert!(
        kernel.test_engine().holds_capability(cap, c),
        "non-cascade must keep C"
    );
}

/// T5: pure WAL replay == checkpoint == live.
#[test]
fn t5_wal_replay_equals_checkpoint_and_live() {
    let wal = temp_wal("t5");
    let kernel = Kernel::with_wal_path(wal.clone());
    let a = birth(&kernel);
    let b = birth(&kernel);
    let c = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);
    delegate(&kernel, cap, a, b, true);
    delegate(&kernel, cap, b, c, false);

    let live_hash = kernel.test_engine().root_hash();
    let live_holds = (
        kernel.test_engine().holds_capability(cap, a),
        kernel.test_engine().holds_capability(cap, b),
        kernel.test_engine().holds_capability(cap, c),
    );
    assert_eq!(live_holds, (true, true, true));

    let snap = kernel.create_checkpoint();
    let wal_ckpt = temp_wal("t5_ckpt");
    let k_ckpt = Kernel::with_wal_path(wal_ckpt);
    assert!(k_ckpt.restore_checkpoint(&snap));
    assert_eq!(
        (
            k_ckpt.test_engine().holds_capability(cap, a),
            k_ckpt.test_engine().holds_capability(cap, b),
            k_ckpt.test_engine().holds_capability(cap, c),
        ),
        live_holds
    );

    // Pure WAL recovery
    let k_wal = Kernel::with_wal_path(wal.clone());
    assert_eq!(
        (
            k_wal.test_engine().holds_capability(cap, a),
            k_wal.test_engine().holds_capability(cap, b),
            k_wal.test_engine().holds_capability(cap, c),
        ),
        live_holds,
        "WAL recovery must restore full delegate topology"
    );
    assert_eq!(
        k_wal.test_engine().root_hash(),
        live_hash,
        "WAL replay root_hash must match live"
    );
}

/// T6: rollback_to drops pending delegates.
#[test]
fn t6_rollback_drops_pending_delegate() {
    let kernel = Kernel::with_wal_path(temp_wal("t6"));
    let a = birth(&kernel);
    let b = birth(&kernel);
    let res = birth(&kernel);
    let cap = grant(&kernel, a, res);

    let mut tx = kernel.test_begin();
    kernel
        .handle(
            &mut tx,
            KernelCall::Savepoint {
                name: "sp".into(),
            },
        )
        .unwrap();
    kernel
        .handle(
            &mut tx,
            KernelCall::CapabilityDelegate {
                capability_id: cap,
                from: a,
                to: b,
                cascade_on_revoke: true,
            },
        )
        .unwrap();
    assert_eq!(tx.pending_delegates.len(), 1);
    kernel
        .handle(
            &mut tx,
            KernelCall::RollbackTo {
                name: "sp".into(),
            },
        )
        .unwrap();
    assert!(
        tx.pending_delegates.is_empty(),
        "rollback must clear pending delegates"
    );
    // Commit empty residual — graph must not gain holder b
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    assert!(!kernel.test_engine().holds_capability(cap, b));
    assert!(kernel.test_engine().holds_capability(cap, a));
}

/// T7: old WAL without CAPDELEGATE still deserializes (empty delegates).
#[test]
fn t7_old_wal_without_capdelegate_compatible() {
    use veritas_kernel::types::{
        PendingCapabilityGrant, TransactionDelta,
    };
    let delta = TransactionDelta {
        tx_id: 1,
        commit_version: 1,
        actor_id: 0,
        writes: vec![],
        scope_changes: vec![],
        births: vec![],
        deaths: vec![],
        freezes: vec![],
        links: vec![],
        unlinks: vec![],
        capability_grants: vec![PendingCapabilityGrant {
            capability_id: 42,
            grant_sequence: 1,
            cap_type: "read".into(),
            grantor: 1,
            grantee: 1,
            resource: 9,
        }],
        capability_delegates: vec![],
        capability_revokes: vec![],
        effects: vec![],
    };
    // Serialize without any CAPDELEGATE tokens (empty vec).
    let s = delta.serialize();
    assert!(!s.contains("CAPDELEGATE"));
    let back = TransactionDelta::deserialize(&s).expect("old-style delta must parse");
    assert!(back.capability_delegates.is_empty());
    assert_eq!(back.capability_grants.len(), 1);
}
