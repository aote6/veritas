use crate::state_memory::StateSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub state_root: u64,
    pub state_version: u64,
    pub snapshot: StateSnapshot,
}

impl Checkpoint {
    pub fn new(snapshot: StateSnapshot) -> Self {
        Self { state_root: snapshot.root_hash, state_version: snapshot.version, snapshot }
    }

    pub fn verify(&self) -> bool {
        self.snapshot.root_hash == self.state_root
            && self.snapshot.version == self.state_version
    }
}
