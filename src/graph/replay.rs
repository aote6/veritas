use crate::graph::journal::{GraphJournal, GraphJournalRecord};
use crate::graph::store::GraphStore;
use crate::graph::types::EdgeId;
use crate::types::TxId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayMode {
    Strict,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    UnbalancedTransaction { tx_id: TxId },
    TransactionMismatch { expected: TxId, got: TxId },
}

pub struct ReplayEngine;

impl ReplayEngine {
    pub fn replay(
        journal: &GraphJournal,
        target_store: &mut GraphStore,
    ) -> Result<(), ReplayError> {
        Self::replay_with_mode(journal, target_store, ReplayMode::Strict)
    }

    pub fn replay_with_mode(
        journal: &GraphJournal,
        target_store: &mut GraphStore,
        mode: ReplayMode,
    ) -> Result<(), ReplayError> {
        let mut active_tx: Option<TxId> = None;
        let mut staging_adds = Vec::new();
        let mut staging_removes: Vec<EdgeId> = Vec::new();

        for record in journal.records() {
            match record {
                GraphJournalRecord::BeginTx(tx_id) => {
                    if let Some(active) = active_tx {
                        return Err(ReplayError::UnbalancedTransaction { tx_id: active });
                    }
                    active_tx = Some(*tx_id);
                    staging_adds.clear();
                    staging_removes.clear();
                }
                GraphJournalRecord::AddEdge { tx_id, edge } => {
                    match active_tx {
                        Some(active) if active == *tx_id => {
                            staging_adds.push(edge.clone());
                        }
                        Some(active) => {
                            return Err(ReplayError::TransactionMismatch {
                                expected: active,
                                got: *tx_id,
                            });
                        }
                        None => {
                            return Err(ReplayError::UnbalancedTransaction { tx_id: *tx_id });
                        }
                    }
                }
                GraphJournalRecord::RemoveEdge { tx_id, edge_id } => {
                    match active_tx {
                        Some(active) if active == *tx_id => {
                            staging_removes.push(*edge_id);
                        }
                        Some(active) => {
                            return Err(ReplayError::TransactionMismatch {
                                expected: active,
                                got: *tx_id,
                            });
                        }
                        None => {
                            return Err(ReplayError::UnbalancedTransaction { tx_id: *tx_id });
                        }
                    }
                }
                GraphJournalRecord::CommitTx(tx_id) => {
                    match active_tx {
                        Some(active) if active == *tx_id => {
                            for edge_id in staging_removes.drain(..) {
                                target_store.remove_edge(edge_id);
                            }
                            for edge in staging_adds.drain(..) {
                                target_store.add_edge(edge);
                            }
                            active_tx = None;
                        }
                        Some(active) => {
                            return Err(ReplayError::TransactionMismatch {
                                expected: active,
                                got: *tx_id,
                            });
                        }
                        None => {
                            return Err(ReplayError::UnbalancedTransaction { tx_id: *tx_id });
                        }
                    }
                }
                GraphJournalRecord::AbortTx(tx_id) => {
                    match active_tx {
                        Some(active) if active == *tx_id => {
                            staging_adds.clear();
                            staging_removes.clear();
                            active_tx = None;
                        }
                        Some(active) => {
                            return Err(ReplayError::TransactionMismatch {
                                expected: active,
                                got: *tx_id,
                            });
                        }
                        None => {
                            return Err(ReplayError::UnbalancedTransaction { tx_id: *tx_id });
                        }
                    }
                }
            }
        }

        // EOF 未闭合事务处理策略
        if let Some(active) = active_tx {
            match mode {
                ReplayMode::Strict => {
                    return Err(ReplayError::UnbalancedTransaction { tx_id: active });
                }
                ReplayMode::Recovery => {
                    // 崩溃恢复策略：自动丢弃未完成事务的 staging 操作，返回一致的状态
                    staging_adds.clear();
                    staging_removes.clear();
                }
            }
        }

        Ok(())
    }
}
