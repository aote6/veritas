use veritas_kernel::graph::{Edge, EdgeId, GraphJournal, GraphPolicyError, GraphStore, GraphTransaction, LinkType};
use veritas_kernel::types::ObjectState;

#[test]
fn g3a_1_staging_isolation_and_rollback() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();
    let mut tx = GraphTransaction::new(1);

    let edge = Edge {
        id: EdgeId(1),
        from: 10,
        to: 20,
        kind: LinkType::Owns,
    };

    tx.add_edge(edge.clone());
    assert_eq!(store.lookup_edge(EdgeId(1)), None);

    tx.rollback();
    let get_state = |_| Some(ObjectState::Alive);
    assert!(tx.commit(&mut store, &mut journal, get_state).is_ok());
    assert_eq!(store.lookup_edge(EdgeId(1)), None);
    assert!(journal.is_empty());
}

#[test]
fn g3a_2_commit_success() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();
    let mut tx = GraphTransaction::new(2);

    let edge = Edge {
        id: EdgeId(100),
        from: 1,
        to: 2,
        kind: LinkType::References,
    };

    tx.add_edge(edge.clone());
    let get_state = |_| Some(ObjectState::Alive);
    assert!(tx.commit(&mut store, &mut journal, get_state).is_ok());

    assert_eq!(store.lookup_edge(EdgeId(100)), Some(&edge));
}

#[test]
fn g3a_3_commit_policy_rejection_atomic_rollback() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();
    let mut tx = GraphTransaction::new(3);

    let bad_edge = Edge {
        id: EdgeId(200),
        from: 1,
        to: 99,
        kind: LinkType::Owns,
    };

    tx.add_edge(bad_edge);

    let get_state = |id| match id {
        1 => Some(ObjectState::Alive),
        99 => Some(ObjectState::Dead),
        _ => None,
    };

    assert_eq!(
        tx.commit(&mut store, &mut journal, get_state),
        Err(GraphPolicyError::NodeDead(99))
    );
    assert_eq!(store.lookup_edge(EdgeId(200)), None);
    assert!(journal.is_empty());
}
