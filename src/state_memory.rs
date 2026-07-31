// Veritas Kernel - State Memory
// 键从裸StateId改为Address(ObjectId, StateId)寻址。
//
// 已知技术债（未在本次改动中处理）：本模块与engine.rs的state_store
// 是两份独立的存储，靠apply_state_memory手动同步。这是迭代早期
// state_root需求晚到、临时拼接留下的架构缺陷，不是本次改动的目标，
// 后续应合并为单一数据源。详见STATUS.md已知限制。

use std::collections::BTreeMap;
use crate::types::Address;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    pub entries: BTreeMap<Address, Vec<u8>>,
    pub root_hash: u64,
    pub version: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StateMemory {
    entries: BTreeMap<Address, Vec<u8>>,
    global_version: u64,
}

impl StateMemory {
    pub fn new() -> Self { Self::default() }

    pub fn write(&mut self, addr: Address, payload: Vec<u8>) {
        self.entries.insert(addr, payload);
        self.global_version += 1;
    }

    pub fn read(&self, addr: Address) -> Option<&Vec<u8>> {
        self.entries.get(&addr)
    }

    pub fn root_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for (addr, val) in &self.entries {
            h ^= addr.object_id;
            h = h.wrapping_mul(0x100000001b3);
            h ^= addr.state_id;
            h = h.wrapping_mul(0x100000001b3);
            for &byte in val {
                h ^= byte as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    pub fn version(&self) -> u64 { self.global_version }

    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            entries: self.entries.clone(),
            root_hash: self.root_hash(),
            version: self.global_version,
        }
    }

    pub fn restore(&mut self, snap: &StateSnapshot) {
        self.entries = snap.entries.clone();
        self.global_version = snap.version;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_write_read() {
        let mut sm = StateMemory::new();
        sm.write(Address::new(0, 0xFFFFFFFFFFFFFF01), vec![1, 2, 3]);
        assert_eq!(sm.read(Address::new(0, 0xFFFFFFFFFFFFFF01)), Some(&vec![1, 2, 3]));
    }

    #[test]
    fn test_deterministic_hash() {
        let mut a = StateMemory::new();
        a.write(Address::new(0, 100), vec![10, 20]);
        let mut b = StateMemory::new();
        b.write(Address::new(0, 100), vec![10, 20]);
        assert_eq!(a.root_hash(), b.root_hash());
    }
}
