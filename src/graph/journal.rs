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
        Self::default()
    }

    /// 底层追加接口：将一条 WAL Record 追加到日志流末尾
    pub fn record(&mut self, record: GraphJournalRecord) {
        self.records.push(record);
    }

    /// 兼容别名接口，方便上层 Transaction/Recovery 模块无缝调用
    #[inline]
    pub fn append(&mut self, record: GraphJournalRecord) {
        self.record(record);
    }

    pub fn records(&self) -> &[GraphJournalRecord] {
        &self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}
