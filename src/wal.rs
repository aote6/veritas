use crate::types::CapabilityId;
// Veritas Kernel V0.3 - WAL 模块
// P1: Scope 结构变更持久化 + Effect 幂等确认与重试

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::types::{Address, ObjectId, ScopeChangeType, ScopeEntry, ScopeId, StateEntry, StateId, TransactionDelta, TxId, Version};

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
        writes: Vec<(Address, Vec<u8>)>,
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
        link_type: u8,
    },
    ObjectDeath {
        tx_id: TxId,
        object_id: ObjectId,
    },
    CapabilityGrant {
        tx_id: TxId,
        cap_type: String,
        grantor: ObjectId,
        grantee: ObjectId,
        resource: ObjectId,
        capability_id: CapabilityId,
        grant_sequence: u64,
    },
    ObjectFreeze {
        tx_id: TxId,
        object_id: ObjectId,
    },
    ObjectUnlink {
        tx_id: TxId,
        from: ObjectId,
        to: ObjectId,
    },
    TransactionCommitted(TransactionDelta),
}

impl WalEntry {
    pub fn serialize(&self) -> String {
        let payload = self.serialize_payload();
        let crc = crc32fast::hash(payload.as_bytes());
        format!("LEN={} CRC={:08x} {}", payload.len(), crc, payload)
    }

    fn serialize_payload(&self) -> String {
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
                    line.push_str(&format!(
                        " WRITE {} {} {}",
                        state_id.object_id,
                        state_id.state_id,
                        hex::encode(val)
                    ));
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
            WalEntry::ObjectLink { tx_id, from, to, link_type } => {
                format!("OBJECTLINK TX={} FROM={} TO={} KIND={} END
", tx_id, from, to, link_type)
            }
            WalEntry::ObjectDeath { tx_id, object_id } => {
                format!("OBJECTDEATH TX={} OBJECT={} END
", tx_id, object_id)
            }
            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource, grant_sequence, capability_id } => {
                format!(
                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} cap_id={} SEQ={} END\n",
                    tx_id, cap_type, grantor, grantee, resource, capability_id, grant_sequence
                )
            }
            WalEntry::ObjectFreeze { tx_id, object_id } => {
                format!("OBJECTFREEZE TX={} OBJECT={} END
", tx_id, object_id)
            }
            WalEntry::ObjectUnlink { tx_id, from, to } => {
                format!("OBJECTUNLINK TX={} FROM={} TO={} END
", tx_id, from, to)
            }
            WalEntry::TransactionCommitted(delta) => {
                let inner = delta.serialize();
                format!("{} END
", inner)  // inner already has END, but we wrap it
            }
        }
    }

    pub fn deserialize(line: &str) -> Option<Self> {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed.starts_with('#') {
            return None;
        }

        let mut tokens = line_trimmed.splitn(3, ' ');
        let len_part = tokens.next()?;
        let crc_part = tokens.next()?;
        let payload = tokens.next()?;

        if !len_part.starts_with("LEN=") || !crc_part.starts_with("CRC=") {
            return None;
        }
        let crc_str = crc_part.strip_prefix("CRC=")?;
        let crc_expected = u32::from_str_radix(crc_str, 16).ok()?;

        // lines() strips trailing \n, but serialize() CRC includes it
        let crc_actual = crc32fast::hash(payload.as_bytes());
        if crc_actual != crc_expected {
            let mut payload_with_nl = payload.to_string();
            if !payload_with_nl.ends_with('\n') {
                payload_with_nl.push('\n');
            }
            if crc32fast::hash(payload_with_nl.as_bytes()) != crc_expected {
                return None;
            }
        }
        let parts: Vec<&str> = payload.split_whitespace().collect();
        if parts.is_empty() || parts.last() != Some(&"END") {
            return None;
        }

        match parts[0] {
            "COMMIT" => Self::deserialize_commit(&parts),
            "EFFECTACK" => Self::deserialize_effect_ack(&parts),
            "CHECKPOINT" => Self::deserialize_checkpoint(&parts),
            "OBJECTBIRTH" => Self::deserialize_object_birth(&parts),
            "OBJECTLINK" => Self::deserialize_object_link(&parts),
            "OBJECTDEATH" => Self::deserialize_object_death(&parts),
            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),
            "OBJECTFREEZE" => Self::deserialize_object_freeze(&parts),
            "OBJECTUNLINK" => Self::deserialize_object_unlink(&parts),
            "TXCOMMIT" => Self::deserialize_transaction_committed(payload),
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
                "WRITE" if i + 3 < parts.len() => {
                    let object_id = parts[i + 1].parse::<ObjectId>().ok()?;
                    let state_id = parts[i + 2].parse::<StateId>().ok()?;
                    let val = hex::decode(parts[i + 3]).ok()?;

                    writes.push((
                        Address::new(object_id, state_id),
                        val
                    ));

                    i += 4;
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

    fn deserialize_capability_grant(parts: &[&str]) -> Option<Self> {
        let tx_id = parts
            .iter()
            .find(|p| p.starts_with("TX="))?
            .strip_prefix("TX=")?
            .parse::<TxId>()
            .ok()?;
        let cap_type = parts
            .iter()
            .find(|p| p.starts_with("TYPE="))?
            .strip_prefix("TYPE=")?
            .to_string();
        let grantor = parts
            .iter()
            .find(|p| p.starts_with("GRANTOR="))?
            .strip_prefix("GRANTOR=")?
            .parse::<ObjectId>()
            .ok()?;
        let grantee = parts
            .iter()
            .find(|p| p.starts_with("GRANTEE="))?
            .strip_prefix("GRANTEE=")?
            .parse::<ObjectId>()
            .ok()?;
        let resource = parts
            .iter()
            .find(|p| p.starts_with("RESOURCE="))?
            .strip_prefix("RESOURCE=")?
            .parse::<ObjectId>()
            .ok()?;
        let capability_id = parts
            .iter()
            .find(|p| p.starts_with("cap_id="))?
            .strip_prefix("cap_id=")?
            .parse::<CapabilityId>()
            .ok()?;
        let grant_sequence = parts
            .iter()
            .find(|p| p.starts_with("SEQ="))?
            .strip_prefix("SEQ=")?
            .parse::<u64>()
            .ok()
            .unwrap_or(0);
        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource, capability_id, grant_sequence })
    }

    fn deserialize_object_freeze(parts: &[&str]) -> Option<Self> {
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
        Some(WalEntry::ObjectFreeze { tx_id, object_id })
    }

    fn deserialize_object_unlink(parts: &[&str]) -> Option<Self> {
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
        Some(WalEntry::ObjectUnlink { tx_id, from, to })
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
        let link_type = parts
            .iter()
            .find(|p| p.starts_with("KIND="))?
            .strip_prefix("KIND=")?
            .parse::<u8>()
            .ok()?;
        Some(WalEntry::ObjectLink { tx_id, from, to, link_type })
    }

    fn deserialize_transaction_committed(payload: &str) -> Option<Self> {
        TransactionDelta::deserialize(payload).map(WalEntry::TransactionCommitted)
    }

    fn deserialize_object_death(parts: &[&str]) -> Option<Self> {
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
        Some(WalEntry::ObjectDeath { tx_id, object_id })
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
            match WalEntry::deserialize(&line) {
                Some(entry) => {
                    match &entry {
                        WalEntry::Commit { version, .. } | WalEntry::Checkpoint { version } => {
                            if *version > max_version {
                                max_version = *version;
                            }
                        }
                        WalEntry::ObjectBirth { .. } => {}
                        WalEntry::ObjectLink { .. } => {}
                        WalEntry::ObjectDeath { .. } => {}
                        _ => {}
                    }
                    records.push(entry);
                }
                None => break,
            }
        }
        Ok((records, max_version))
    }

    pub fn apply_records(
        records: &[WalEntry],
    ) -> (
        HashMap<Address, StateEntry>,
        HashMap<ScopeId, ScopeEntry>,
        Vec<PendingRecoveryEffect>,
        TxId,
    ) {
        let mut state_map: HashMap<Address, StateEntry> = HashMap::new();
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
                    for (addr, value) in writes {
                        state_map.insert(
                            *addr,
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
                WalEntry::ObjectDeath { .. } => {}
                WalEntry::CapabilityGrant { .. } => {}
                WalEntry::ObjectFreeze { .. } => {}
                WalEntry::ObjectUnlink { .. } => {}
                WalEntry::TransactionCommitted(delta) => {
                    if delta.tx_id > max_tx_id {
                        max_tx_id = delta.tx_id;
                    }
                    for (addr, value) in &delta.writes {
                        state_map.insert(
                            *addr,
                            StateEntry {
                                value: value.clone(),
                                version: delta.commit_version,
                            },
                        );
                    }
                    for (scope_id, change_type, state_id) in &delta.scope_changes {
                        let entry = scope_map
                            .entry(*scope_id)
                            .or_insert_with(ScopeEntry::new);
                        match change_type {
                            ScopeChangeType::Bind => { entry.bind(*state_id); }
                            ScopeChangeType::Unbind => { entry.unbind(*state_id); }
                        }
                    }
                    for (key, payload) in &delta.effects {
                        committed_effects.push(PendingRecoveryEffect {
                            tx_id: delta.tx_id,
                            idempotency_key: key.clone(),
                            payload: payload.clone(),
                        });
                    }
                }
            }
        }

        let pending: Vec<PendingRecoveryEffect> = committed_effects
            .into_iter()
            .filter(|e| !acked_keys.contains(&e.idempotency_key))
            .collect();

        (state_map, scope_map, pending, max_tx_id)
    }
}
/// 从 WAL records 按 tx_id 分组构建 TransactionDelta 列表。
/// 只保留有 Commit marker 的事务，丢弃孤儿条目。
/// Recovery 和 Replay 共用此函数。
pub(crate) fn build_ordered_deltas(
    records: &[WalEntry],
) -> Vec<TransactionDelta> {
    use std::collections::HashMap;
    use crate::types::{LinkType, PendingCapabilityGrant, TransactionDelta};

    let mut partial_deltas: HashMap<TxId, TransactionDelta> = HashMap::new();
    let mut ordered_deltas: Vec<TransactionDelta> = Vec::new();

    for record in records {
        match record {
            WalEntry::ObjectBirth { tx_id, object_id } => {
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.births.push(*object_id);
            }
            WalEntry::ObjectDeath { tx_id, object_id } => {
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.deaths.push(*object_id);
            }
            WalEntry::ObjectFreeze { tx_id, object_id } => {
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.freezes.push(*object_id);
            }
            WalEntry::ObjectLink { tx_id, from, to, link_type, .. } => {
                let relation = match link_type {
                    0 => LinkType::DependsOn,
                    1 => LinkType::Owns,
                    2 => LinkType::References,
                    _ => continue,
                };
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.links.push((*from, *to, relation));
            }
            WalEntry::ObjectUnlink { tx_id, from, to } => {
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.unlinks.push((*from, *to));
            }
            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource, capability_id, grant_sequence } => {
                let delta = partial_deltas.entry(*tx_id).or_insert_with(|| TransactionDelta {
                    actor_id: 0,
                tx_id: *tx_id, commit_version: 0,
                    writes: vec![], scope_changes: vec![],
                    births: vec![], deaths: vec![], freezes: vec![],
                    links: vec![], unlinks: vec![],
                    capability_grants: vec![], capability_delegates: vec![], capability_revokes: vec![], effects: vec![],
                });
                delta.capability_grants.push(PendingCapabilityGrant {
                    capability_id: *capability_id,
                    grant_sequence: *grant_sequence,
                    cap_type: cap_type.clone(),
                    grantor: *grantor,
                    grantee: *grantee,
                    resource: *resource,
                });
            }
            WalEntry::Commit { tx_id, version, writes, scope_changes, effects } => {
                if let std::collections::hash_map::Entry::Occupied(mut entry) = partial_deltas.entry(*tx_id) {
                    let delta = entry.get_mut();
                    delta.commit_version = *version;
                    delta.writes = writes.iter().map(|(addr, val)| (*addr, val.clone())).collect();
                    delta.scope_changes = scope_changes.iter().map(|c| (c.scope_id, c.change_type.clone(), c.state_id)).collect();
                    delta.effects = effects.iter().map(|e| (e.idempotency_key.clone(), e.payload.clone())).collect();
                    ordered_deltas.push(delta.clone());
                    entry.remove();
                }
            }
            WalEntry::TransactionCommitted(delta) => {
                ordered_deltas.push(delta.clone());
                partial_deltas.remove(&delta.tx_id);
            }
            _ => {}
        }
    }
    // 丢弃留在 partial_deltas 中的无 Commit marker 的事务

    ordered_deltas
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
            writes: vec![(Address::new(0,100), vec![10,11,12]), (Address::new(0,200), vec![255,238])],
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
    fn test_crc_mismatch_rejected() {
        let entry = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![],
            scope_changes: vec![],
            effects: vec![],
        };
        let good = entry.serialize();
        assert!(WalEntry::deserialize(&good).is_some());

        // 篡改 payload 中的一个字节
        let tampered = good.replace("COMMIT", "XCOMMIT");
        assert!(WalEntry::deserialize(&tampered).is_none());
    }

    #[test]
    fn test_truncated_with_crc() {
        let entry = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![],
            scope_changes: vec![],
            effects: vec![],
        };
        let full = entry.serialize();
        // 截断到一半
        let half = &full[..full.len() / 2];
        assert!(WalEntry::deserialize(half).is_none());
    }

    #[test]
    fn test_truncated_at_len_prefix() {
        // 模拟文件末尾只写了 "LEN=" 就断电
        assert!(WalEntry::deserialize("LEN=").is_none());
        assert!(WalEntry::deserialize("LEN=123").is_none());
    }

    #[test]
    fn test_truncated_at_crc_prefix() {
        // 模拟文件末尾写到 CRC= 就断电
        assert!(WalEntry::deserialize("LEN=123 CRC=").is_none());
        assert!(WalEntry::deserialize("LEN=123 CRC=abc").is_none());
    }

    #[test]
    fn test_transaction_delta_roundtrip() {
        use crate::types::{Address, LinkType, PendingCapabilityGrant, ScopeChangeType, TransactionDelta};

        let delta = TransactionDelta {
            actor_id: 0,
                tx_id: 42,
            commit_version: 3,
            writes: vec![
                (Address::new(10, 100), vec![1, 2, 3]),
                (Address::new(20, 200), vec![255, 0, 128]),
            ],
            scope_changes: vec![
                (55, ScopeChangeType::Bind, 1),
                (66, ScopeChangeType::Unbind, 2),
            ],
            births: vec![100, 200],
            deaths: vec![300],
            freezes: vec![400],
            links: vec![
                (10, 20, LinkType::Owns),
                (30, 40, LinkType::DependsOn),
            ],
            unlinks: vec![(50, 60)],
            capability_grants: vec![
                PendingCapabilityGrant {
                    capability_id: 999,
                    grant_sequence: 5,
                    cap_type: "AdminCap".to_string(),
                    grantor: 1,
                    grantee: 2,
                    resource: 3,
                },
            ],
            capability_delegates: vec![],
            capability_revokes: vec![],
            effects: vec![
                ("key-1".to_string(), vec![7, 8, 9]),
            ],
        };

        let serialized = delta.serialize();
        let deserialized = TransactionDelta::deserialize(&serialized);
        assert!(deserialized.is_some(), "deserialize returned None for: {}", serialized);
        let d2 = deserialized.unwrap();

        assert_eq!(d2.tx_id, delta.tx_id);
        assert_eq!(d2.commit_version, delta.commit_version);
        assert_eq!(d2.writes, delta.writes);
        assert_eq!(d2.scope_changes, delta.scope_changes);
        assert_eq!(d2.births, delta.births);
        assert_eq!(d2.deaths, delta.deaths);
        assert_eq!(d2.freezes, delta.freezes);
        assert_eq!(d2.links, delta.links);
        assert_eq!(d2.unlinks, delta.unlinks);
        assert_eq!(d2.capability_grants.len(), 1);
        assert_eq!(d2.capability_grants[0].capability_id, 999);
        assert_eq!(d2.capability_grants[0].grant_sequence, 5);
        assert_eq!(d2.capability_grants[0].cap_type, "AdminCap");
        assert_eq!(d2.effects.len(), 1);
        assert_eq!(d2.effects[0].0, "key-1");
        assert_eq!(d2.effects[0].1, vec![7, 8, 9]);
    }

    #[test]
    fn test_truncated_transaction_committed_discarded() {
        use crate::types::{Address, TransactionDelta};

        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        // 1. Write a complete TransactionCommitted
        let writer = WalWriter::open(path).unwrap();
        let delta = TransactionDelta {
            actor_id: 0,
                tx_id: 1,
            commit_version: 1,
            writes: vec![],
            scope_changes: vec![],
            births: vec![100],
            deaths: vec![],
            freezes: vec![],
            links: vec![],
            unlinks: vec![],
            capability_grants: vec![],
            capability_delegates: vec![],
            capability_revokes: vec![],
            effects: vec![],
        };
        let entry = WalEntry::TransactionCommitted(delta);
        writer.append_and_sync(&entry).unwrap();

        // 2. Append a corrupted entry with deliberately wrong CRC header
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap();
        use std::io::Write;
        writeln!(file, "LEN=50 CRC=00000000 TXCOMMIT TX=2 VERSION=1 BIRTH 200 END").unwrap();

        // 3. Recover: only the first entry should survive
        let (records, _) = RecoveryManager::recover(path).unwrap();
        assert_eq!(records.len(), 1, "only the valid entry should be recovered");
        match &records[0] {
            WalEntry::TransactionCommitted(d) => {
                assert_eq!(d.births, vec![100]);
            }
            _ => panic!("expected TransactionCommitted"),
        }
    }

    #[test]
    fn test_wal_write_and_read() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();
        let writer = WalWriter::open(path).unwrap();
        let entry = WalEntry::Commit {
            tx_id: 1,
            version: 1,
            writes: vec![(Address::new(0,42), vec![1,2,3])],
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
                writes: vec![(Address::new(0, i), vec![i as u8])],
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
            let writer = WalWriter::open(path).unwrap();
            let entry = WalEntry::Commit {
                tx_id: 1,
                version: 1,
                writes: vec![(Address::new(0, 100), vec![10, 11])],
                scope_changes: vec![],
                effects: vec![],
            };
            writer.append_and_sync(&entry).unwrap();

            // Write a corrupted line with wrong CRC
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "LEN=20 CRC=12345678 COMMIT TX=2 BROKEN").unwrap();
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
            writes: vec![(Address::new(0,1), vec![9])],
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
            writes: vec![(Address::new(0,1), vec![9])],
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