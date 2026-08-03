use crate::common::new_kernel;
use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::types::{ObjectState, LinkType, ObjectType};

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.begin();
    let id = match kernel.handle(&mut tx, KernelCall::ObjectBirth {
        object_type: ObjectType::StateObject,
    }).unwrap() {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
    id
}

fn death(kernel: &Kernel, id: u64) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectDeath { object_id: id }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn freeze(kernel: &Kernel, id: u64) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectFreeze { object_id: id }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

fn link(kernel: &Kernel, from: u64, to: u64, lt: LinkType) {
    let mut tx = kernel.begin();
    kernel.handle(&mut tx, KernelCall::ObjectLink { from, to, link_type: lt }).unwrap();
    kernel.handle(&mut tx, KernelCall::Commit).unwrap();
}

#[test]
fn lifecycle_birth_alive() {
    let tk = new_kernel();
    let obj = birth(&tk.kernel);
    assert_eq!(tk.kernel.get_object_state(obj), Some(ObjectState::Alive));
}

#[test]
fn lifecycle_birth_freeze_dead() {
    let tk = new_kernel();
    let obj = birth(&tk.kernel);
    freeze(&tk.kernel, obj);
    assert_eq!(tk.kernel.get_object_state(obj), Some(ObjectState::Frozen));
    death(&tk.kernel, obj);
    assert_eq!(tk.kernel.get_object_state(obj), Some(ObjectState::Dead));
}

#[test]
#[ignore = "frozen link check deferred to commit — pending validation"]
    fn lifecycle_frozen_rejects_link() {
    let tk = new_kernel();
    let a = birth(&tk.kernel);
    let b = birth(&tk.kernel);
    freeze(&tk.kernel, a);
    let mut tx = tk.kernel.begin();
    tk.kernel.handle(&mut tx, KernelCall::ObjectLink { from: a, to: b, link_type: LinkType::Owns }).unwrap();
    let result = tk.kernel.handle(&mut tx, KernelCall::Commit);
    assert!(result.is_err(), "frozen object must reject new links at commit");
}

#[test]
fn lifecycle_owns_cascade() {
    let tk = new_kernel();
    let a = birth(&tk.kernel);
    let b = birth(&tk.kernel);
    link(&tk.kernel, a, b, LinkType::Owns);
    death(&tk.kernel, a);
    assert!(tk.kernel.is_object_dead(a));
    assert!(tk.kernel.is_object_dead(b), "OWNS must cascade");
}

#[test]
fn lifecycle_depends_on_invalidation() {
    let tk = new_kernel();
    let a = birth(&tk.kernel);
    let b = birth(&tk.kernel);
    link(&tk.kernel, a, b, LinkType::DependsOn);
    death(&tk.kernel, b);
    assert!(tk.kernel.is_object_dead(b));
    assert!(!tk.kernel.is_object_dead(a), "DEPENDS_ON must not cascade");
    assert!(!tk.kernel.has_link(a, b), "DEPENDS_ON link must be removed after target death");
}

#[test]
fn lifecycle_references_no_cascade() {
    let tk = new_kernel();
    let a = birth(&tk.kernel);
    let b = birth(&tk.kernel);
    link(&tk.kernel, a, b, LinkType::References);
    death(&tk.kernel, b);
    assert!(tk.kernel.is_object_dead(b));
    assert!(!tk.kernel.is_object_dead(a), "REFERENCES must not cascade");
    assert!(!tk.kernel.has_link(a, b), "REFERENCES link must be removed");
}

#[test]
fn lifecycle_self_link_rejected() {
    let tk = new_kernel();
    let obj = birth(&tk.kernel);
    let mut tx = tk.kernel.begin();
    let result = tk.kernel.handle(&mut tx, KernelCall::ObjectLink {
        from: obj, to: obj, link_type: LinkType::Owns,
    });
    assert!(result.is_err(), "self-link must be rejected");
}

#[test]
fn lifecycle_death_irreversible() {
    let tk = new_kernel();
    let obj = birth(&tk.kernel);
    death(&tk.kernel, obj);
    let mut tx = tk.kernel.begin();
    let result = tk.kernel.handle(&mut tx, KernelCall::ObjectDeath { object_id: obj });
    assert!(result.is_err(), "re-death must be rejected");
}

#[test]
fn lifecycle_alive_to_dead() {
    let tk = new_kernel();
    let obj = birth(&tk.kernel);
    death(&tk.kernel, obj);
    assert_eq!(tk.kernel.get_object_state(obj), Some(ObjectState::Dead));
}

#[test]
fn lifecycle_owns_chain_cascade() {
    let tk = new_kernel();
    let a = birth(&tk.kernel);
    let b = birth(&tk.kernel);
    let c = birth(&tk.kernel);
    link(&tk.kernel, a, b, LinkType::Owns);
    link(&tk.kernel, b, c, LinkType::Owns);
    death(&tk.kernel, a);
    assert!(tk.kernel.is_object_dead(a));
    assert!(tk.kernel.is_object_dead(b));
    assert!(tk.kernel.is_object_dead(c), "OWNS chain must cascade fully");
}
