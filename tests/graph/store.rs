use veritas_kernel::graph::{Edge, EdgeId, GraphStore, LinkType};

#[test]
fn g1_1_add_and_lookup_edge() {
    let mut store = GraphStore::new();
    let edge = Edge {
        id: EdgeId(1),
        from: 10,
        to: 20,
        kind: LinkType::Owns,
    };

    store.add_edge(edge.clone());
    assert_eq!(store.lookup_edge(EdgeId(1)), Some(&edge));
}

#[test]
fn g1_2_remove_edge() {
    let mut store = GraphStore::new();
    let edge = Edge {
        id: EdgeId(2),
        from: 10,
        to: 20,
        kind: LinkType::References,
    };

    store.add_edge(edge.clone());
    assert_eq!(store.remove_edge(EdgeId(2)), Some(edge));
    assert_eq!(store.lookup_edge(EdgeId(2)), None);
}

#[test]
fn g1_3_incoming_outgoing_index() {
    let mut store = GraphStore::new();
    let edge1 = Edge {
        id: EdgeId(10),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };
    let edge2 = Edge {
        id: EdgeId(11),
        from: 1,
        to: 3,
        kind: LinkType::References,
    };

    store.add_edge(edge1.clone());
    store.add_edge(edge2.clone());

    let outs: Vec<&Edge> = store.outgoing_edges(1).collect();
    assert_eq!(outs.len(), 2);

    let ins_2: Vec<&Edge> = store.incoming_edges(2).collect();
    assert_eq!(ins_2.len(), 1);
    assert_eq!(ins_2[0], &edge1);
}
