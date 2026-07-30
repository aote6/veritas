#[derive(Debug, Clone)]
pub struct Memory {
    data: Vec<u8>,
}

impl Memory {
    pub fn new(capacity: usize) -> Self {
        Self { data: vec![0; capacity] }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn write_bytes(&mut self, addr: usize, bytes: &[u8]) -> Result<(), String> {
        if addr + bytes.len() > self.data.len() {
            return Err(format!("Memory OOB write: addr 0x{:X} len {}", addr, bytes.len()));
        }
        self.data[addr..addr + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub fn slice_from(&self, addr: usize) -> Result<&[u8], String> {
        if addr >= self.data.len() {
            return Err(format!("Memory OOB read: addr 0x{:X}", addr));
        }
        Ok(&self.data[addr..])
    }

    pub fn clear(&mut self) {
        self.data.fill(0);
    }
}
