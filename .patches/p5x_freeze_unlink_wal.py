#!/usr/bin/env python3
import shutil, datetime

def backup(path):
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak_{ts}"
    shutil.copy(path, bak)
    print(f"[backup] {path} -> {bak}")

def apply(path, edits):
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    backup(path)
    for desc, old, new in edits:
        count = content.count(old)
        if count != 1:
            print(f"[FAIL] {path}: '{desc}' 锚点出现 {count} 次,跳过整个文件")
            return False
        content = content.replace(old, new, 1)
        print(f"[OK] {path}: {desc}")
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    return True

# ---------------- src/wal.rs ----------------
wal_edits = [
    ("enum 加 ObjectFreeze / ObjectUnlink 变体",
     '''    CapabilityGrant {
        tx_id: TxId,
        cap_type: String,
        grantor: ObjectId,
        grantee: ObjectId,
        resource: ObjectId,
    },
}''',
     '''    CapabilityGrant {
        tx_id: TxId,
        cap_type: String,
        grantor: ObjectId,
        grantee: ObjectId,
        resource: ObjectId,
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
}'''),

    ("serialize 加 ObjectFreeze / ObjectUnlink 分支",
     '''            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource } => {
                format!(
                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} END\\n",
                    tx_id, cap_type, grantor, grantee, resource
                )
            }
        }
    }''',
     '''            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource } => {
                format!(
                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} END\\n",
                    tx_id, cap_type, grantor, grantee, resource
                )
            }
            WalEntry::ObjectFreeze { tx_id, object_id } => {
                format!("OBJECTFREEZE TX={} OBJECT={} END\\n", tx_id, object_id)
            }
            WalEntry::ObjectUnlink { tx_id, from, to } => {
                format!("OBJECTUNLINK TX={} FROM={} TO={} END\\n", tx_id, from, to)
            }
        }
    }'''),

    ("deserialize 分派表加 OBJECTFREEZE / OBJECTUNLINK",
     '''            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),
            _ => None,
        }''',
     '''            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),
            "OBJECTFREEZE" => Self::deserialize_object_freeze(&parts),
            "OBJECTUNLINK" => Self::deserialize_object_unlink(&parts),
            _ => None,
        }'''),

    ("加 deserialize_object_freeze / deserialize_object_unlink 函数",
     '''        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource })
    }

    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {''',
     '''        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource })
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

    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {'''),
]

# ---------------- src/engine.rs ----------------
engine_edits = [
    ("commit(): Freeze 落地前先写 WAL,再改状态(原来只改状态,没写WAL)",
     '''        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_deaths {
                if let Some(r) = registry.get_mut(object_id) { r.state = crate::types::ObjectState::Dead; } else { let mut r = crate::types::ObjectRecord::new_state(*object_id); r.state = crate::types::ObjectState::Dead; registry.insert(*object_id, r); }
            }
            for object_id in ctx.pending_freezes.drain(..) {
                if let Some(r) = registry.get_mut(&object_id) { r.state = crate::types::ObjectState::Frozen; } else { let mut r = crate::types::ObjectRecord::new_state(object_id); r.state = crate::types::ObjectState::Frozen; registry.insert(object_id, r); }
            }
        }''',
     '''        // P5.x: 写入 ObjectFreeze WAL 条目(修复:此前 Freeze 只改内存 registry,
        // 从未写 WAL,重启后 Frozen 状态会静默丢失,写保护失效)
        let freezes_to_apply: Vec<ObjectId> = ctx.pending_freezes.drain(..).collect();
        for object_id in &freezes_to_apply {
            let freeze_entry = WalEntry::ObjectFreeze {
                tx_id: ctx.tx_id(),
                object_id: *object_id,
            };
            self.wal
                .append_and_sync(&freeze_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL ObjectFreeze write failed: {}", e)))?;
        }
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_deaths {
                if let Some(r) = registry.get_mut(object_id) { r.state = crate::types::ObjectState::Dead; } else { let mut r = crate::types::ObjectRecord::new_state(*object_id); r.state = crate::types::ObjectState::Dead; registry.insert(*object_id, r); }
            }
            for object_id in freezes_to_apply {
                if let Some(r) = registry.get_mut(&object_id) { r.state = crate::types::ObjectState::Frozen; } else { let mut r = crate::types::ObjectRecord::new_state(object_id); r.state = crate::types::ObjectState::Frozen; registry.insert(object_id, r); }
            }
        }'''),

    ("commit(): Unlink 落地前先写 WAL(原来只改内存 topology,没写WAL)",
     '''        {
            let mut topo = self.topology.lock().unwrap();
            for edge in &ctx.pending_links {
                topo.push(edge.clone());
            }
            // P26: 处理 pending_unlinks
            for (from, to) in &ctx.pending_unlinks {
                topo.retain(|e| e.from != *from || e.to != *to);
            }
        }''',
     '''        // P5.x: 写入 ObjectUnlink WAL 条目(修复:此前 Unlink 只改内存 topology,
        // 从未写 WAL,重启后被解除的边会静默复活,可能引发错误的死亡级联)
        for (from, to) in &ctx.pending_unlinks {
            let unlink_entry = WalEntry::ObjectUnlink {
                tx_id: ctx.tx_id(),
                from: *from,
                to: *to,
            };
            self.wal
                .append_and_sync(&unlink_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL ObjectUnlink write failed: {}", e)))?;
        }
        {
            let mut topo = self.topology.lock().unwrap();
            for edge in &ctx.pending_links {
                topo.push(edge.clone());
            }
            // P26: 处理 pending_unlinks
            for (from, to) in &ctx.pending_unlinks {
                topo.retain(|e| e.from != *from || e.to != *to);
            }
        }'''),

    ("recovery: WAL 循环里处理 ObjectFreeze / ObjectUnlink",
     '''                WalEntry::CapabilityGrant { cap_type, grantor, grantee, resource, .. } => {
                    recovered_cap_grants.push((cap_type.clone(), *grantor, *grantee, *resource));
                }
                _ => {}
            }
        }''',
     '''                WalEntry::CapabilityGrant { cap_type, grantor, grantee, resource, .. } => {
                    recovered_cap_grants.push((cap_type.clone(), *grantor, *grantee, *resource));
                }
                WalEntry::ObjectFreeze { object_id, .. } => {
                    if let Some(r) = recovered_objects.get_mut(object_id) {
                        r.state = crate::types::ObjectState::Frozen;
                    } else {
                        let mut r = crate::types::ObjectRecord::new_state(*object_id);
                        r.state = crate::types::ObjectState::Frozen;
                        recovered_objects.insert(*object_id, r);
                    }
                }
                WalEntry::ObjectUnlink { from, to, .. } => {
                    recovered_links.retain(|edge| edge.from != *from || edge.to != *to);
                }
                _ => {}
            }
        }'''),
]

ok = True
ok &= apply("src/wal.rs", wal_edits)
ok &= apply("src/engine.rs", engine_edits)

test_file_content = '''use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{LinkType, ObjectState};

/// P5.x: Object Freeze 后 commit,重启(WAL recovery)后状态必须仍是 Frozen。
/// 修复前:Freeze 只改内存 registry,从未写 WAL,重启后静默退回 Alive,
/// 写保护失效。
#[test]
fn freeze_survives_recovery() {
    let wal_path = format!("target/test_freeze_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let target: u64 = 0xF2EEZE01;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, target).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_freeze(&mut tx2, target).unwrap();
        engine.commit(&mut tx2).unwrap();

        assert_eq!(engine.get_object_state(target), Some(ObjectState::Frozen));
    }

    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());
    assert_eq!(
        recovered_engine.get_object_state(target),
        Some(ObjectState::Frozen),
        "Frozen state must survive engine restart via WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}

/// P5.x: Object Unlink 后 commit,重启后这条边必须仍然是解除状态,
/// 不能复活。修复前:Unlink 只改内存 topology,从未写 WAL。
#[test]
fn unlink_survives_recovery() {
    let wal_path = format!("target/test_unlink_recovery_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let a: u64 = 0xUNL1NK01u64 as u64;
    let b: u64 = 0xUNL1NK02u64 as u64;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, a).unwrap();
        engine.object_birth(&mut tx, b).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        engine.object_link(&mut tx2, a, b, LinkType::References).unwrap();
        engine.commit(&mut tx2).unwrap();

        let mut tx3 = engine.begin();
        engine.object_unlink(&mut tx3, a, b).unwrap();
        engine.commit(&mut tx3).unwrap();
    }

    let recovered_engine = VeritasEngine::with_wal_path(wal_path.clone());
    assert!(
        !recovered_engine.has_link(a, b),
        "Unlinked edge must not reappear after WAL recovery"
    );

    let _ = std::fs::remove_file(&wal_path);
}
'''

with open("tests/freeze_unlink_p5x_recovery.rs", "w", encoding="utf-8") as f:
    f.write(test_file_content)
print("[OK] 已创建 tests/freeze_unlink_p5x_recovery.rs (占位,函数名可能需要根据实际API调整)")

if ok:
    print("\n先跑: cargo build 2>&1 | tail -60")
else:
    print("\n有 [FAIL],先解决 wal.rs/engine.rs 的锚点问题,测试文件本次不要跑")
