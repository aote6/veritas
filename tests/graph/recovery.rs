use veritas_kernel::graph::journal::{GraphJournal, GraphJournalRecord};
use veritas_kernel::graph::recovery::GraphRecovery;
use veritas_kernel::graph::types::{Edge, EdgeId, LinkType};

fn make_edge(id: u64, from: u64, to: u64) -> Edge {
    Edge {
        id: EdgeId(id),
        from,
        to,
        kind: LinkType::References,
    }
}

#[test]
fn g4_1_recovery_empty_journal() {
    let journal = GraphJournal::new();
    let store = GraphRecovery::recover(&journal).unwrap();
    assert_eq!(store.edge_count(), 0);
}

#[test]
fn g4_2_recovery_committed_tx() {
    let mut journal = GraphJournal::new();
    let tx = 100u64;
    let edge = make_edge(1, 10, 20);

    journal.record(GraphJournalRecord::BeginTx(tx));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx, edge: edge.clone() });
    journal.record(GraphJournalRecord::CommitTx(tx));

    let store = GraphRecovery::recover(&journal).unwrap();
    assert_eq!(store.edge_count(), 1);
    assert_eq!(store.lookup_edge(EdgeId(1)), Some(&edge));
}

#[test]
fn g4_3_recovery_uncommitted_tx_crash() {
    let mut journal = GraphJournal::new();
    let tx = 100u64;
    let edge = make_edge(1, 10, 20);

    // 崩溃场景：写入了 BeginTx 与 AddEdge，但尚未写入 CommitTx 即发生 Crash
    journal.record(GraphJournalRecord::BeginTx(tx));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx, edge });

    let store = GraphRecovery::recover(&journal).unwrap();
    // 恢复后未提交的事务应当被废弃，Store 保持绝对干净
    assert_eq!(store.edge_count(), 0);
    assert_eq!(store.lookup_edge(EdgeId(1)), None);
}

#[test]
fn g4_4_recovery_aborted_tx() {
    let mut journal = GraphJournal::new();
    let tx = 100u64;
    let edge = make_edge(1, 10, 20);

    journal.record(GraphJournalRecord::BeginTx(tx));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx, edge });
    journal.record(GraphJournalRecord::AbortTx(tx));

    let store = GraphRecovery::recover(&journal).unwrap();
    assert_eq!(store.edge_count(), 0);
}

#[test]
fn g4_5_recovery_multiple_transactions() {
    let mut journal = GraphJournal::new();
    let tx1 = 1u64;
    let tx2 = 2u64;
    let tx3 = 3u64;

    let e1 = make_edge(1, 10, 20);
    let e2 = make_edge(2, 20, 30);
    let e3 = make_edge(3, 30, 40);

    // Tx1 成功提交
    journal.record(GraphJournalRecord::BeginTx(tx1));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx1, edge: e1.clone() });
    journal.record(GraphJournalRecord::CommitTx(tx1));

    // Tx2 Abort
    journal.record(GraphJournalRecord::BeginTx(tx2));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx2, edge: e2 });
    journal.record(GraphJournalRecord::AbortTx(tx2));

    // Tx3 成功提交
    journal.record(GraphJournalRecord::BeginTx(tx3));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx3, edge: e3.clone() });
    journal.record(GraphJournalRecord::CommitTx(tx3));

    let store = GraphRecovery::recover(&journal).unwrap();
    assert_eq!(store.edge_count(), 2);
    assert_eq!(store.lookup_edge(EdgeId(1)), Some(&e1));
    assert_eq!(store.lookup_edge(EdgeId(2)), None);
    assert_eq!(store.lookup_edge(EdgeId(3)), Some(&e3));
}

#[test]
fn g4_6_recovery_idempotency() {
    let mut journal = GraphJournal::new();
    let tx = 100u64;
    let edge = make_edge(1, 10, 20);

    journal.record(GraphJournalRecord::BeginTx(tx));
    journal.record(GraphJournalRecord::AddEdge { tx_id: tx, edge: edge.clone() });
    journal.record(GraphJournalRecord::CommitTx(tx));

    let store1 = GraphRecovery::recover(&journal).unwrap();
    let store2 = GraphRecovery::recover(&journal).unwrap();

    assert_eq!(store1, store2);
    assert_eq!(store1.edge_count(), 1);
}
