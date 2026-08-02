use veritas_kernel::graph::{Edge, EdgeId, GraphPolicy, GraphPolicyError, GraphStore, LinkType};
use veritas_kernel::types::ObjectState;

#[test]
fn g2_1_canonical_edge_uniqueness() {
    let mut store = GraphStore::new();
    let edge1 = Edge {
        id: EdgeId(1),
        from: 10,
        to: 20,
        kind: LinkType::Owns,
    };

    store.add_edge(edge1.clone());

    let get_state = |_| Some(ObjectState::Alive);

    // 重复添加相同的 (from, to, kind) 必须触发 CanonicalConflict
    let dup_edge = Edge {
        id: EdgeId(2),
        from: 10,
        to: 20,
        kind: LinkType::Owns,
    };

    assert_eq!(
        GraphPolicy::validate(&store, &dup_edge, get_state),
        Err(GraphPolicyError::CanonicalConflict { proposed_from: 10, proposed_to: 20 })
    );
}

#[test]
fn g2_2_boundary_node_existence_and_death() {
    let store = GraphStore::new();

    let valid_edge = Edge {
        id: EdgeId(1),
        from: 1,
        to: 2,
        kind: LinkType::References,
    };

    let get_state_valid = |_| Some(ObjectState::Alive);
    assert!(GraphPolicy::validate(&store, &valid_edge, get_state_valid).is_ok());

    let get_state_dead = |id| match id {
        1 => Some(ObjectState::Alive),
        2 => Some(ObjectState::Dead),
        _ => None,
    };

    assert_eq!(
        GraphPolicy::validate(&store, &valid_edge, get_state_dead),
        Err(GraphPolicyError::NodeDead(2))
    );
}

#[test]
fn g2_3_owns_dag_cycle_prevention() {
    let mut store = GraphStore::new();
    let edge1 = Edge {
        id: EdgeId(1),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };
    store.add_edge(edge1);

    let get_state = |_| Some(ObjectState::Alive);

    // 添加 2 -> 1 的 Owns 边必须检测出拓扑环
    let cycle_edge = Edge {
        id: EdgeId(2),
        from: 2,
        to: 1,
        kind: LinkType::Owns,
    };

    assert_eq!(
        GraphPolicy::validate(&store, &cycle_edge, get_state),
        Err(GraphPolicyError::OwnsCycleDetected { proposed_from: 2, proposed_to: 1 })
    );
}
