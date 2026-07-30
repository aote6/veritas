use crate::types::StateId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRecord {
    pub tx_id: u64,
    pub capability_id: Option<u64>,
    pub writes: Vec<(StateId, Vec<u8>)>,
    pub before_root: u64,
    pub after_root: u64,
}

impl ReplayRecord {
    pub fn new(tx_id: u64, capability_id: Option<u64>, writes: Vec<(StateId, Vec<u8>)>, before_root: u64, after_root: u64) -> Self {
        Self { tx_id, capability_id, writes, before_root, after_root }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionHistory {
    pub records: Vec<ReplayRecord>,
}

impl ExecutionHistory {
    pub fn new() -> Self { Self::default() }
    pub fn push(&mut self, r: ReplayRecord) { self.records.push(r); }
    pub fn records(&self) -> &[ReplayRecord] { &self.records }
}
