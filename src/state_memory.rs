use std::collections::BTreeMap;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone)]
pub struct StatePage {
    pub data: Box<[u8; PAGE_SIZE]>,
    pub version: u64,
}

impl StatePage {
    pub fn new() -> Self {
        Self { data: Box::new([0u8; PAGE_SIZE]), version: 0 }
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in self.data.iter() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

#[derive(Debug, Clone)]
pub struct StateMemory {
    pages: BTreeMap<u64, StatePage>,
    global_version: u64,
}

impl StateMemory {
    pub fn new() -> Self {
        Self { pages: BTreeMap::new(), global_version: 0 }
    }

    pub fn write(&mut self, addr: u64, data: &[u8]) {
        let page_id = addr / PAGE_SIZE as u64;
        let offset = (addr % PAGE_SIZE as u64) as usize;
        let page = self.pages.entry(page_id).or_insert_with(StatePage::new);
        let end = (offset + data.len()).min(PAGE_SIZE);
        page.data[offset..end].copy_from_slice(&data[..end - offset]);
        page.version += 1;
        self.global_version += 1;
    }

    pub fn read(&self, addr: u64, len: usize) -> Vec<u8> {
        let page_id = addr / PAGE_SIZE as u64;
        let offset = (addr % PAGE_SIZE as u64) as usize;
        match self.pages.get(&page_id) {
            Some(page) => {
                let end = (offset + len).min(PAGE_SIZE);
                page.data[offset..end].to_vec()
            }
            None => vec![0u8; len],
        }
    }

    pub fn root_hash(&self) -> u64 {
        let mut h = 0u64;
        for (id, page) in &self.pages {
            h ^= id ^ page.hash();
        }
        h
    }

    pub fn version(&self) -> u64 {
        self.global_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_memory_write_read() {
        let mut sm = StateMemory::new();
        sm.write(0, &[1, 2, 3, 4]);
        assert_eq!(sm.read(0, 4), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_state_memory_version_changes() {
        let mut sm = StateMemory::new();
        let v1 = sm.version();
        sm.write(100, &[0xFF]);
        assert!(sm.version() > v1);
    }

    #[test]
    fn test_state_memory_root_hash_deterministic() {
        let mut sm1 = StateMemory::new();
        sm1.write(0, &[1, 2, 3]);

        let mut sm2 = StateMemory::new();
        sm2.write(0, &[1, 2, 3]);

        assert_eq!(sm1.root_hash(), sm2.root_hash());
    }
}
