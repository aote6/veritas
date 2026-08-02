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

NL = "\n"  # 实际换行符,用于拼出和文件一致的内嵌换行 format! 字符串

wal_edits = [
    ("enum 加 ObjectFreeze / ObjectUnlink 变体",
     "    CapabilityGrant {\n        tx_id: TxId,\n        cap_type: String,\n        grantor: ObjectId,\n        grantee: ObjectId,\n        resource: ObjectId,\n    },\n}",
     "    CapabilityGrant {\n        tx_id: TxId,\n        cap_type: String,\n        grantor: ObjectId,\n        grantee: ObjectId,\n        resource: ObjectId,\n    },\n    ObjectFreeze {\n        tx_id: TxId,\n        object_id: ObjectId,\n    },\n    ObjectUnlink {\n        tx_id: TxId,\n        from: ObjectId,\n        to: ObjectId,\n    },\n}"),

    ("serialize 加 ObjectFreeze / ObjectUnlink 分支",
     '            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource } => {\n'
     '                format!(\n'
     '                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} END\n'
     '",\n'
     '                    tx_id, cap_type, grantor, grantee, resource\n'
     '                )\n'
     '            }\n'
     '        }\n'
     '    }',
     '            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource } => {\n'
     '                format!(\n'
     '                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} END\n'
     '",\n'
     '                    tx_id, cap_type, grantor, grantee, resource\n'
     '                )\n'
     '            }\n'
     '            WalEntry::ObjectFreeze { tx_id, object_id } => {\n'
     '                format!("OBJECTFREEZE TX={} OBJECT={} END\n'
     '", tx_id, object_id)\n'
     '            }\n'
     '            WalEntry::ObjectUnlink { tx_id, from, to } => {\n'
     '                format!("OBJECTUNLINK TX={} FROM={} TO={} END\n'
     '", tx_id, from, to)\n'
     '            }\n'
     '        }\n'
     '    }'),

    ("deserialize 分派表加 OBJECTFREEZE / OBJECTUNLINK",
     '            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),\n            _ => None,\n        }',
     '            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),\n            "OBJECTFREEZE" => Self::deserialize_object_freeze(&parts),\n            "OBJECTUNLINK" => Self::deserialize_object_unlink(&parts),\n            _ => None,\n        }'),

    ("加 deserialize_object_freeze / deserialize_object_unlink 函数",
     '        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource })\n    }\n\n    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {',
     '        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource })\n    }\n\n'
     '    fn deserialize_object_freeze(parts: &[&str]) -> Option<Self> {\n'
     '        let tx_id = parts\n'
     '            .iter()\n'
     '            .find(|p| p.starts_with("TX="))?\n'
     '            .strip_prefix("TX=")?\n'
     '            .parse::<TxId>()\n'
     '            .ok()?;\n'
     '        let object_id = parts\n'
     '            .iter()\n'
     '            .find(|p| p.starts_with("OBJECT="))?\n'
     '            .strip_prefix("OBJECT=")?\n'
     '            .parse::<ObjectId>()\n'
     '            .ok()?;\n'
     '        Some(WalEntry::ObjectFreeze { tx_id, object_id })\n'
     '    }\n\n'
     '    fn deserialize_object_unlink(parts: &[&str]) -> Option<Self> {\n'
     '        let tx_id = parts\n'
     '            .iter()\n'
     '            .find(|p| p.starts_with("TX="))?\n'
     '            .strip_prefix("TX=")?\n'
     '            .parse::<TxId>()\n'
     '            .ok()?;\n'
     '        let from = parts\n'
     '            .iter()\n'
     '            .find(|p| p.starts_with("FROM="))?\n'
     '            .strip_prefix("FROM=")?\n'
     '            .parse::<ObjectId>()\n'
     '            .ok()?;\n'
     '        let to = parts\n'
     '            .iter()\n'
     '            .find(|p| p.starts_with("TO="))?\n'
     '            .strip_prefix("TO=")?\n'
     '            .parse::<ObjectId>()\n'
     '            .ok()?;\n'
     '        Some(WalEntry::ObjectUnlink { tx_id, from, to })\n'
     '    }\n\n'
     '    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {'),
]

ok = apply("src/wal.rs", wal_edits)
if ok:
    print("\nwal.rs 全部锚点匹配成功。")
else:
    print("\nwal.rs 仍有锚点失败,不要动 engine.rs,先把上面 [FAIL] 信息贴出来")
