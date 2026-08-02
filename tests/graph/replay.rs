use veritas_kernel::graph::{
    Edge, EdgeId, GraphJournal, GraphJournalRecord, GraphStore, GraphTransaction, LinkType,
    ReplayEngine,
};
use veritas_kernel::types::ObjectState;

fn get_alive_state(_: u64) -> Option<ObjectState> {
    Some(ObjectState::Alive)
}

#[test]
fn g3c_1_empty_replay() {
    let journal = GraphJournal::new();
    let mut replayed_store = GraphStore::new();

    assert!(ReplayEngine::replay(&journal, &mut replayed_store).is_ok());
    assert!(replayed_store.is_empty());
    assert_eq!(replayed_store.edge_count(), 0);
}

#[test]
fn g3c_2_sequential_replay() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();
    let mut tx = GraphTransaction::new(101);

    let edge = Edge {
        id: EdgeId(1),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };
    tx.add_edge(edge.clone());
    tx.commit(&mut store, &mut journal, get_alive_state).unwrap();

    let mut replayed_store = GraphStore::new();
    assert!(ReplayEngine::replay(&journal, &mut replayed_store).is_ok());

    assert_eq!(replayed_store.edge_count(), 1);
    assert_eq!(replayed_store.lookup_edge(EdgeId(1)), Some(&edge));
}

#[test]
fn g3c_3_multiple_transactions_replay() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();

    let mut tx1 = GraphTransaction::new(1);
    let edge1 = Edge {
        id: EdgeId(10),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };
    tx1.add_edge(edge1.clone());
    tx1.commit(&mut store, &mut journal, get_alive_state).unwrap();

    let mut tx2 = GraphTransaction::new(2);
    let edge2 = Edge {
        id: EdgeId(20),
        from: 2,
        to: 3,
        kind: LinkType::References,
    };
    tx2.add_edge(edge2.clone());
    tx2.commit(&mut store, &mut journal, get_alive_state).unwrap();

    let mut replayed_store = GraphStore::new();
    assert!(ReplayEngine::replay(&journal, &mut replayed_store).is_ok());

    assert_eq!(replayed_store.edge_count(), 2);
    assert_eq!(replayed_store.lookup_edge(EdgeId(10)), Some(&edge1));
    assert_eq!(replayed_store.lookup_edge(EdgeId(20)), Some(&edge2));
}

#[test]
fn g3c_4_replay_determinism_invariant() {
    let mut store_a = GraphStore::new();
    let mut journal = GraphJournal::new();

    for i in 1..=5 {
        let mut tx = GraphTransaction::new(i);
        tx.add_edge(Edge {
            id: EdgeId(i),
            from: i,
            to: i + 10,
            kind: LinkType::References,
        });
        tx.commit(&mut store_a, &mut journal, get_alive_state).unwrap();
    }

    let mut store_b = GraphStore::new();
    ReplayEngine::replay(&journal, &mut store_b).unwrap();

    assert_eq!(store_a.edge_count(), store_b.edge_count());
    let edges_a: Vec<&Edge> = store_a.all_edges().collect();
    let edges_b: Vec<&Edge> = store_b.all_edges().collect();
    assert_eq!(edges_a, edges_b);
}

#[test]
fn g3c_5_remove_replay() {
    let mut journal = GraphJournal::new();

    // 模拟 Tx 1: Add Edge 100
    let edge = Edge {
        id: EdgeId(100),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };
    journal.record(GraphJournalRecord::BeginTx(1));
    journal.record(GraphJournalRecord::AddEdge { tx_id: 1, edge });
    journal.record(GraphJournalRecord::CommitTx(1));

    // 模拟 Tx 2: Remove Edge 100
    journal.record(GraphJournalRecord::BeginTx(2));
    journal.record(GraphJournalRecord::RemoveEdge { tx_id: 2, edge_id: EdgeId(100) });
    journal.record(GraphJournalRecord::CommitTx(2));

    let mut replayed_store = GraphStore::new();
    assert!(ReplayEngine::replay(&journal, &mut replayed_store).is_ok());
    assert_eq!(replayed_store.edge_count(), 0);
    assert_eq!(replayed_store.lookup_edge(EdgeId(100)), None);
}

#[test]
fn g3c_6_abort_replay() {
    let mut journal = GraphJournal::new();

    // 模拟 Tx 1: Add Edge 然后 Abort
    let edge = Edge {
        id: EdgeId(200),
        from: 5,
        to: 6,
        kind: LinkType::References,
    };
    journal.record(GraphJournalRecord::BeginTx(1));
    journal.record(GraphJournalRecord::AddEdge { tx_id: 1, edge });
    journal.record(GraphJournalRecord::AbortTx(1));

    let mut replayed_store = GraphStore::new();
    assert!(ReplayEngine::replay(&journal, &mut replayed_store).is_ok());
    assert_eq!(replayed_store.edge_count(), 0);
}
