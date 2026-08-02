use veritas_kernel::graph::{Edge, EdgeId, GraphJournal, GraphJournalRecord, GraphStore, GraphTransaction, LinkType};
use veritas_kernel::types::ObjectState;

#[test]
fn g3b_1_journal_event_sequence() {
    let mut store = GraphStore::new();
    let mut journal = GraphJournal::new();
    let mut tx = GraphTransaction::new(42);

    let edge = Edge {
        id: EdgeId(10),
        from: 1,
        to: 2,
        kind: LinkType::Owns,
    };

    tx.add_edge(edge.clone());

    let get_state = |_| Some(ObjectState::Alive);
    assert!(tx.commit(&mut store, &mut journal, get_state).is_ok());

    let records = journal.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0], GraphJournalRecord::BeginTx(42));
    assert_eq!(
        records[1],
        GraphJournalRecord::AddEdge {
            tx_id: 42,
            edge: edge.clone()
        }
    );
    assert_eq!(records[2], GraphJournalRecord::CommitTx(42));
}
