// Veritas Kernel V0.2 - WAL 模块

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::types::{StateId, StateEntry, TxId, Version};

#[derive(Debug, Clone, PartialEq)]
pub struct WalRecord {
    pub tx_id: TxId,
    pub version: Version,
    pub writes: Vec<(StateId, Vec<u8>)>,
}

impl WalRecord {
    pub fn serialize(&self) -> String {
        let mut line = format!("TX={} VERSION={}", self.tx_id, self.version);
        for (state_id, val) in &self.writes {
            let hex_val = hex::encode(val);
            line.push_str(&format!(" WRITE {} {}", state_id, hex_val));
        }
        line.push_str(" END\n");
        line
    }

    pub fn deserialize(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 || parts.last() != Some(&"END") {
            return None;
        }

        let tx_part = parts.iter().find(|p| p.starts_with("TX="))?;
        let tx_id = tx_part.strip_prefix("TX=")?.parse::<TxId>().ok()?;

        let ver_part = parts.iter().find(|p| p.starts_with("VERSION="))?;
        let version = ver_part.strip_prefix("VERSION=")?.parse::<Version>().ok()?;

        let mut writes = Vec::new();
        let mut i = 0;
        while i < parts.len() {
            if parts[i] == "WRITE" && i + 2 < parts.len() {
                let state_id = parts[i + 1].parse::<StateId>().ok()?;
                let val = hex::decode(parts[i + 2]).ok()?;
                writes.push((state_id, val));
                i += 3;
            } else {
                i += 1;
            }
        }

        Some(WalRecord { tx_id, version, writes })
    }
}

pub struct WalWriter {
    file: Mutex<File>,
}

impl WalWriter {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }

    pub fn append_and_sync(&self, record: &WalRecord) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        let content = record.serialize();
        file.write_all(content.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }
}

pub struct RecoveryManager;

impl RecoveryManager {
    pub fn recover<P: AsRef<Path>>(path: P) -> io::Result<(Vec<WalRecord>, Version)> {
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(e) => return Err(e),
        };

        let reader = BufReader::new(file);
        let mut records = Vec::new();
        let mut max_version = 0;

        for line_result in reader.lines() {
            let line = line_result?;
            if let Some(record) = WalRecord::deserialize(&line) {
                if record.version > max_version {
                    max_version = record.version;
                }
                records.push(record);
            }
        }

        Ok((records, max_version))
    }

    pub fn apply_records(
        records: &[WalRecord],
        state_store: &mut HashMap<StateId, StateEntry>,
    ) {
        for record in records {
            for (state_id, value) in &record.writes {
                state_store.insert(
                    *state_id,
                    StateEntry {
                        value: value.clone(),
                        version: record.version,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_serialize_roundtrip() {
        let record = WalRecord {
            tx_id: 8,
            version: 15,
            writes: vec![(100, vec![10, 11, 12]), (200, vec![255, 238])],
        };
        let serialized = record.serialize();
        let deserialized = WalRecord::deserialize(&serialized);
        assert!(deserialized.is_some());
        assert_eq!(deserialized.unwrap(), record);
    }

    #[test]
    fn test_incomplete_line() {
        assert!(WalRecord::deserialize("TX=1 VERSION=2 WRITE 100 0A0B").is_none());
        assert!(WalRecord::deserialize("").is_none());
        assert!(WalRecord::deserialize("# comment").is_none());
    }

    #[test]
    fn test_wal_write_and_read() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let record = WalRecord { tx_id: 1, version: 1, writes: vec![(42, vec![1, 2, 3])] };
        writer.append_and_sync(&record).unwrap();

        let (records, max_version) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], record);
        assert_eq!(max_version, 1);
    }

    #[test]
    fn test_multiple_records_recovery() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        for i in 1..=3 {
            let record = WalRecord { tx_id: i, version: i, writes: vec![(i, vec![i as u8])] };
            writer.append_and_sync(&record).unwrap();
        }
        let (records, max_version) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(max_version, 3);
    }

    #[test]
    fn test_empty_wal_recovery() {
        let tmp = NamedTempFile::new().unwrap();
        let (records, max_version) = RecoveryManager::recover(tmp.path()).unwrap();
        assert!(records.is_empty());
        assert_eq!(max_version, 0);
    }

    #[test]
    fn test_corrupted_wal_recovery() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        {
            let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            writeln!(file, "TX=1 VERSION=1 WRITE 100 0A0B END").unwrap();
            write!(file, "TX=2 VERSION=2 WRITE 200").unwrap();
        }
        let (records, max_version) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tx_id, 1);
        assert_eq!(max_version, 1);
    }
}
