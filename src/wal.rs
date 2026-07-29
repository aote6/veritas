// Veritas Kernel V0.3 - WAL 模块
// P1: Scope 结构变更持久化 + Effect 幂等确认与重试

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::types::{ObjectId, ScopeChangeType, ScopeEntry, ScopeId, StateEntry, StateId, TxId, Version};

#[derive(Debug, Clone, PartialEq)]
pub struct WalScopeChange {
    pub scope_id: ScopeId,
    pub change_type: ScopeChangeType,
    pub state_id: StateId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WalEffect {
    pub idempotency_key: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WalEntry {
    Commit {
        tx_id: TxId,
        version: Version,
        writes: Vec<(StateId, Vec<u8>)>,
        scope_changes: Vec<WalScopeChange>,
        effects: Vec<WalEffect>,
    },
    EffectAck {
        tx_id: TxId,
        idempotency_key: String,
    },
    Checkpoint {
        version: Version,
    },
    ObjectBirth {
        tx_id: TxId,
        object_id: ObjectId,
    },
    ObjectLink {
        tx_id: TxId,
        from: ObjectId,
        to: ObjectId,
        relation_kind: u8,
    },
}

impl WalEntry {
    pub fn serialize(&self) -> String {
        match self {
            WalEntry::Commit {
                tx_id,
                version,
                writes,
                scope_changes,
                effects,
            } => {
                let mut line = format!("COMMIT TX={} VERSION={}", tx_id, version);
                for (state_id, val) in writes {
                    line.push_str(&format!(" WRITE {} {}", state_id, hex::encode(val)));
                }
                for change in scope_changes {
                    let tag = match change.change_type {
                        ScopeChangeType::Bind => "SCOPEBIND",
                        ScopeChangeType::Unbind => "SCOPEUNBIND",
                    };
                    line.push_str(&format!(
                        " {} {} {}",
                        tag, change.scope_id, change.state_id
                    ));
                }
                for effect in effects {
                    line.push_str(&format!(
                        " EFFECT {} {}",
                        effect.idempotency_key,
                        hex::encode(&effect.payload)
                    ));
                }
                line.push_str(" END\n");
                line
            }
            WalEntry::EffectAck {
                tx_id,
                idempotency_key,
            } => {
                format!("EFFECTACK TX={} KEY={} END\n", tx_id, idempotency_key)
            }
            WalEntry::Checkpoint { version } => {
                format!("CHECKPOINT VERSION={} END
", version)
            }
            WalEntry::ObjectBirth { tx_id, object_id } => {
                format!("OBJECTBIRTH TX={} OBJECT={} END
", tx_id, object_id)
            }
            WalEntry::ObjectLink { tx_id, from, to, relation_kind } => {
                format!("OBJECTLINK TX={} FROM={} TO={} KIND={} END
", tx_id, from, to, relation_kind)
            }
        }
    }

    pub fn deserialize(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts.last() != Some(&"END") {
            return None;
        }

        match parts[0] {
            "COMMIT" => Self::deserialize_commit(&parts),
            "EFFECTACK" => Self::deserialize_effect_ack(&parts),
            "CHECKPOINT" => Self::deserialize_checkpoint(&parts),
            "OBJECTBIRTH" => Self::deserialize_object_birth(&parts),
            "OBJECTLINK" => Self::deserialize_object_link(&parts),
            _ => None,
        }
    }

    fn deserialize_commit(parts: &[&str]) -> Option<Self> {
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let version = parts
            .iter()
            .find(|p| p.starts_with("VERSION="))?
            .strip_prefix("VERSION=")?
            .parse::<Version>()
            .ok()?;

        let mut writes = Vec::new();
        let mut scope_changes = Vec::new();
        let mut effects = Vec::new();
        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "WRITE" if i + 2 < parts.len() => {
                    let state_id = parts[i + 1].parse::<StateId>().ok()?;
                    let val = hex::decode(parts[i + 2]).ok()?;
                    writes.push((state_id, val));
                    i += 3;
                }
                "SCOPEBIND" if i + 2 < parts.len() => {
                    let scope_id = parts[i + 1].parse::<ScopeId>().ok()?;
                    let state_id = parts[i + 2].parse::<StateId>().ok()?;
                    scope_changes.push(WalScopeChange {
                        scope_id,
                        change_type: ScopeChangeType::Bind,
                        state_id,
                    });
                    i += 3;
                }
                "SCOPEUNBIND" if i + 2 < parts.len() => {
                    let scope_id = parts[i + 1].parse::<ScopeId>().ok()?;
                    let state_id = parts[i + 2].parse::<StateId>().ok()?;
                    scope_changes.push(WalScopeChange {
                        scope_id,
                        change_type: ScopeChangeType::Unbind,
                        state_id,
                    });
                    i += 3;
                }
                "EFFECT" if i + 2 < parts.len() => {
                    let key = parts[i + 1].to_string();
                    let payload = hex::decode(parts[i + 2]).ok()?;
                    effects.push(WalEffect {
                        idempotency_key: key,
                        payload,
                    });
                    i += 3;
                }
                _ => i += 1,
            }
        }

        Some(WalEntry::Commit {
            tx_id,
            version,
            writes,
            scope_changes,
            effects,
        })
    }

    fn deserialize_effect_ack(parts: &[&str]) -> Option<Self> {
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let idempotency_key = parts
            .iter()
            .find(|p| p.starts_with("KEY="))?
            .strip_prefix("KEY=")?
            .to_string();
        Some(WalEntry::EffectAck {
            tx_id,
            idempotency_key,
        })
    }

    fn deserialize_checkpoint(parts: &[&str]) -> Option<Self> {
        let version = parts
            .iter()
            .find(|p| p.starts_with("VERSION="))?
            .strip_prefix("VERSION=")?
            .parse::<Version>()
            .ok()?;
        Some(WalEntry::Checkpoint { version })
    }

    fn deserialize_object_birth(parts: &[&str]) -> Option<Self> {
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let object_id = parts
            .iter()
            .find(|p| p.starts_with("OBJECT="))?
            .strip_prefix("OBJECT=")?
            .parse::<ObjectId>()
            .ok()?;
        Some(WalEntry::ObjectBirth { tx_id, object_id })
    }

    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let from = parts
            .iter()
            .find(|p| p.starts_with("FROM="))?
            .strip_prefix("FROM=")?
            .parse::<ObjectId>()
            .ok()?;
        let to = parts
            .iter()
            .find(|p| p.starts_with("TO="))?
            .strip_prefix("TO=")?
            .parse::<ObjectId>()
            .ok()?;
        let relation_kind = parts
            .iter()
            .find(|p| p.starts_with("KIND="))?
            .strip_prefix("KIND=")?
            .parse::<u8>()
            .ok()?;
        Some(WalEntry::ObjectLink { tx_id, from, to, relation_kind })
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
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    pub fn append_and_sync(&self, entry: &WalEntry) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        let content = entry.serialize();
        file.write_all(content.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }
}

/// 恢复后待重试的副作用
pub struct PendingRecoveryEffect {
    pub tx_id: TxId,
    pub idempotency_key: String,
    pub payload: Vec<u8>,
}

pub struct RecoveryManager;

impl RecoveryManager {
    pub fn recover<P: AsRef<Path>>(path: P) -> io::Result<(Vec<WalEntry>, Version)> {
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
            if let Some(entry) = WalEntry::deserialize(&line) {
                match &entry {
                    WalEntry::Commit { version, .. } | WalEntry::Checkpoint { version } => {
                        if *version > max_version {
                            max_version = *version;
                        }
                    }
                    WalEntry::ObjectBirth { .. } => {}
                    WalEntry::ObjectLink { .. } => {}
                    _ => {}
                }
                records.push(entry);
            }
        }

        Ok((records, max_version))
    }

    pub fn apply_records(
        records: &[WalEntry],
    ) -> (
        HashMap<StateId, StateEntry>,
        HashMap<ScopeId, ScopeEntry>,
        Vec<PendingRecoveryEffect>,
        TxId,
    ) {
        let mut state_map: HashMap<StateId, StateEntry> = HashMap::new();
        let mut scope_map: HashMap<ScopeId, ScopeEntry> = HashMap::new();
        let mut committed_effects: Vec<PendingRecoveryEffect> = Vec::new();
        let mut acked_keys: HashSet<String> = HashSet::new();
        let mut max_tx_id: TxId = 0;

        for record in records {
            match record {
                WalEntry::Commit {
                    tx_id,
                    version,
                    writes,
                    scope_changes,
                    effects,
                } => {
                    if *tx_id > max_tx_id {
                        max_tx_id = *tx_id;
                    }
                    for (state_id, value) in writes {
                        state_map.insert(
                            *state_id,
                            StateEntry {
                                value: value.clone(),
                                version: *version,
                            },
                        );
                    }
                    for change in scope_changes {
                        let entry = scope_map
                            .entry(change.scope_id)
                            .or_insert_with(ScopeEntry::new);
                        match change.change_type {
                            ScopeChangeType::Bind => {
                                entry.bind(change.state_id);
                            }
                            ScopeChangeType::Unbind => {
                                entry.unbind(change.state_id);
                            }
                        }
                    }
                    for effect in effects {
                        committed_effects.push(PendingRecoveryEffect {
                            tx_id: *tx_id,
                            idempotency_key: effect.idempotency_key.clone(),
                            payload: effect.payload.clone(),
                        });
                    }
                }
                WalEntry::EffectAck {
                    idempotency_key, ..
                } => {
                    acked_keys.insert(idempotency_key.clone());
                }
                WalEntry::Checkpoint { .. } => {}
                WalEntry::ObjectBirth { .. } => {}
                WalEntry::ObjectLink { .. } => {}
            }
        }

        let pending: Vec<PendingRecoveryEffect> = committed_effects
            .into_iter()
            .filter(|e| !acked_keys.contains(&e.idempotency_key))
            .collect();

        (state_map, scope_map, pending, max_tx_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_commit_serialize_roundtrip() {
        let entry = WalEntry::Commit {
            tx_id: 8,
            version: 15,
            writes: vec![(100, vec![10, 11, 12]), (200, vec![255, 238])],
            scope_changes: vec![
                WalScopeChange {
                    scope_id: 55,
                    change_type: ScopeChangeType::Bind,
                    state_id: 100,
                },
                WalScopeChange {
                    scope_id: 66,
                    change_type: ScopeChangeType::Unbind,
                    state_id: 200,
                },
            ],
            effects: vec![WalEffect {
                idempotency_key: "8-0".to_string(),
                payload: vec![1, 2, 3],
            }],
        };
        let serialized = entry.serialize();
        assert_eq!(WalEntry::deserialize(&serialized), Some(entry));
    }

    #[test]
    fn test_effect_ack_roundtrip() {
        let entry = WalEntry::EffectAck {
            tx_id: 3,
            idempotency_key: "3-1".to_string(),
        };
        assert_eq!(WalEntry::deserialize(&entry.serialize()), Some(entry));
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let entry = WalEntry::Checkpoint { version: 42 };
        assert_eq!(WalEntry::deserialize(&entry.serialize()), Some(entry));
    }

    #[test]
    fn test_incomplete_line() {
        assert!(WalEntry::deserialize("COMMIT TX=1 VERSION=2 WRITE 100 0A0B").is_none());
        assert!(WalEntry::deserialize("").is_none());
        assert!(WalEntry::deserialize("# comment").is_none());
    }

    #[test]
    fn test_wal_write_and_read() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let entry = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![(42, vec![1, 2, 3])],
            scope_changes: vec![],
            effects: vec![],
        };
        writer.append_and_sync(&entry).unwrap();

        let (records, max_version) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], entry);
        assert_eq!(max_version, 1);
    }

    #[test]
    fn test_multiple_records_recovery() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        for i in 1..=3u64 {
            let entry = WalEntry::Commit {
                tx_id: i,
                version: i,
                writes: vec![(i, vec![i as u8])],
                scope_changes: vec![],
                effects: vec![],
            };
            writer.append_and_sync(&entry).unwrap();
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
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap();
            writeln!(file, "COMMIT TX=1 VERSION=1 WRITE 100 0A0B END").unwrap();
            write!(file, "COMMIT TX=2 VERSION=2 WRITE 200").unwrap();
        }
        let (records, max_version) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 1);
        match &records[0] {
            WalEntry::Commit { tx_id, .. } => assert_eq!(*tx_id, 1),
            _ => panic!("expected commit entry"),
        }
        assert_eq!(max_version, 1);
    }

    #[test]
    fn test_effect_retry_after_crash_without_ack() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let commit = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![(1, vec![9])],
            scope_changes: vec![],
            effects: vec![WalEffect {
                idempotency_key: "1-0".to_string(),
                payload: vec![7],
            }],
        };
        writer.append_and_sync(&commit).unwrap();

        let (records, _) = RecoveryManager::recover(path).unwrap();
        let (_, _, pending, max_tx_id) = RecoveryManager::apply_records(&records);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].idempotency_key, "1-0");
        assert_eq!(max_tx_id, 1);
    }

    #[test]
    fn test_effect_not_retried_if_acked() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let commit = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![(1, vec![9])],
            scope_changes: vec![],
            effects: vec![WalEffect {
                idempotency_key: "1-0".to_string(),
                payload: vec![7],
            }],
        };
        writer.append_and_sync(&commit).unwrap();
        writer
            .append_and_sync(&WalEntry::EffectAck {
                tx_id: 1,
                idempotency_key: "1-0".to_string(),
            })
            .unwrap();

        let (records, _) = RecoveryManager::recover(path).unwrap();
        let (_, _, pending, _) = RecoveryManager::apply_records(&records);
        assert!(pending.is_empty());
    }

    #[test]
    fn test_scope_changes_replayed_into_scope_map() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let commit = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![],
            scope_changes: vec![
                WalScopeChange {
                    scope_id: 100,
                    change_type: ScopeChangeType::Bind,
                    state_id: 1,
                },
                WalScopeChange {
                    scope_id: 100,
                    change_type: ScopeChangeType::Bind,
                    state_id: 2,
                },
            ],
            effects: vec![],
        };
        writer.append_and_sync(&commit).unwrap();

        let (records, _) = RecoveryManager::recover(path).unwrap();
        let (_, scope_map, _, _) = RecoveryManager::apply_records(&records);
        let entry = scope_map.get(&100).unwrap();
        assert_eq!(entry.struct_version, 2);
        assert_eq!(entry.members.len(), 2);
    }
}
