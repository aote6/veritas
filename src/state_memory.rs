use std::collections::BTreeMap;
use crate::types::StateId;

#[derive(Debug, Clone, Default)]
pub struct StateMemory {
    entries: BTreeMap<StateId, Vec<u8>>,
    global_version: u64,
}

impl StateMemory {
    pub fn new() -> Self { Self::default() }

    pub fn write(&mut self, state_id: StateId, payload: Vec<u8>) {
        self.entries.insert(state_id, payload);
        self.global_version += 1;
    }

    pub fn read(&self, state_id: StateId) -> Option<&Vec<u8>> {
        self.entries.get(&state_id)
    }

    pub fn root_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for (id, val) in &self.entries {
            h ^= id;
            h = h.wrapping_mul(0x100000001b3);
            for &byte in val {
                h ^= byte as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    pub fn version(&self) -> u64 { self.global_version }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_write_read() {
        let mut sm = StateMemory::new();
        sm.write(0xFFFFFFFFFFFFFF01, vec![1, 2, 3]);
        assert_eq!(sm.read(0xFFFFFFFFFFFFFF01), Some(&vec![1, 2, 3]));
    }

    #[test]
    fn test_deterministic_hash() {
        let mut a = StateMemory::new();
        a.write(100, vec![10, 20]);
        let mut b = StateMemory::new();
        b.write(100, vec![10, 20]);
        assert_eq!(a.root_hash(), b.root_hash());
    }
}
