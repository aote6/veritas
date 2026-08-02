use crate::graph::journal::{GraphJournal, GraphJournalRecord};
use crate::graph::policy::{GraphPolicy, GraphPolicyError};
use crate::graph::store::GraphStore;
use crate::graph::types::{Edge, EdgeId};
use crate::types::{ObjectId, ObjectState, TxId};
use std::collections::BTreeMap;
use std::mem;

pub struct GraphTransaction {
    tx_id: TxId,
    pending_adds: BTreeMap<EdgeId, Edge>,
    pending_removes: BTreeMap<EdgeId, Edge>,
}

impl GraphTransaction {
    pub fn new(tx_id: TxId) -> Self {
        Self {
            tx_id,
            pending_adds: BTreeMap::new(),
            pending_removes: BTreeMap::new(),
        }
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.pending_removes.remove(&edge.id);
        self.pending_adds.insert(edge.id, edge);
    }

    pub fn remove_edge(&mut self, store: &GraphStore, edge_id: EdgeId) {
        if self.pending_adds.remove(&edge_id).is_some() {
            return;
        }

        if let Some(edge) = store.lookup_edge(edge_id) {
            self.pending_removes.insert(edge_id, edge.clone());
        }
    }

    pub fn rollback(&mut self) {
        self.pending_adds.clear();
        self.pending_removes.clear();
    }

    /// 提交事务：Policy 预检 -> 生效至 Store -> 追加至 Journal
    pub fn commit<F>(
        &mut self,
        store: &mut GraphStore,
        journal: &mut GraphJournal,
        get_state: F,
    ) -> Result<(), GraphPolicyError>
    where
        F: Fn(ObjectId) -> Option<ObjectState> + Copy,
    {
        if self.pending_adds.is_empty() && self.pending_removes.is_empty() {
            return Ok(());
        }
        // 1. Dry-run 预检
        for edge in self.pending_adds.values() {
            GraphPolicy::validate(store, edge, get_state)?;
        }

        // 2. 校验通过，写入 Journal 日志流
        journal.append(GraphJournalRecord::BeginTx(self.tx_id));

        for edge_id in self.pending_removes.keys() {
            journal.append(GraphJournalRecord::RemoveEdge {
                tx_id: self.tx_id,
                edge_id: *edge_id,
            });
            store.remove_edge(*edge_id);
        }

        let adds = mem::take(&mut self.pending_adds);
        for (_, edge) in adds {
            journal.append(GraphJournalRecord::AddEdge {
                tx_id: self.tx_id,
                edge: edge.clone(),
            });
            store.add_edge(edge);
        }

        journal.append(GraphJournalRecord::CommitTx(self.tx_id));

        self.pending_removes.clear();
        Ok(())
    }
}
