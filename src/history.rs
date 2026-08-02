use crate::types::Address;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRecord {
    pub tx_id: u64,
    pub capability_ids: Vec<u64>,
    pub program_hash: u64,
    pub writes: Vec<(Address, Vec<u8>)>,
    pub before_root: u64,
    pub after_root: u64,
}

impl ReplayRecord {
    pub fn new(
        tx_id: u64,
        capability_ids: Vec<u64>,
        program_hash: u64,
        writes: Vec<(Address, Vec<u8>)>,
        before_root: u64,
        after_root: u64,
    ) -> Self {
        Self { tx_id, capability_ids: capability_ids, program_hash, writes, before_root, after_root }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub version: u64,
    pub record: ReplayRecord,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionHistory {
    pub records: Vec<HistoryEntry>,
    next_version: u64,
}

impl ExecutionHistory {
    pub fn new() -> Self { Self { records: Vec::new(), next_version: 1 } }

    pub fn push(&mut self, record: ReplayRecord) -> u64 {
        let v = self.next_version;
        self.records.push(HistoryEntry { version: v, record });
        self.next_version += 1;
        v
    }

    pub fn entries(&self) -> &[HistoryEntry] { &self.records }
}
