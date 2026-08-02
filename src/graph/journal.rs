use crate::graph::types::{Edge, EdgeId};
use crate::types::TxId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphJournalRecord {
    BeginTx(TxId),
    AddEdge { tx_id: TxId, edge: Edge },
    RemoveEdge { tx_id: TxId, edge_id: EdgeId },
    CommitTx(TxId),
    AbortTx(TxId),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GraphJournal {
    records: Vec<GraphJournalRecord>,
}

impl GraphJournal {
    pub fn new() -> Self {
        Self { records: Vec::new() }
    }

    /// 追加一条日志记录
    pub fn append(&mut self, record: GraphJournalRecord) {
        self.records.push(record);
    }

    /// 获取只读日志切片
    pub fn records(&self) -> &[GraphJournalRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
