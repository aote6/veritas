#!/usr/bin/env python3
import shutil, sys, datetime

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
            print(f"[FAIL] {path}: '{desc}' 锚点出现 {count} 次(需要恰好1次),跳过整个文件,请手动检查")
            return False
        content = content.replace(old, new, 1)
        print(f"[OK] {path}: {desc}")
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    return True

# ---------------- src/wal.rs ----------------
wal_edits = [
    ("enum 加 CapabilityGrant 变体",
     '''    ObjectDeath {
        tx_id: TxId,
        object_id: ObjectId,
    },
}''',
     '''    ObjectDeath {
        tx_id: TxId,
        object_id: ObjectId,
    },
    CapabilityGrant {
        tx_id: TxId,
        cap_type: String,
        grantor: ObjectId,
        grantee: ObjectId,
        resource: ObjectId,
    },
}'''),

    ("serialize 加 CapabilityGrant 分支",
     '''            WalEntry::ObjectDeath { tx_id, object_id } => {
                format!("OBJECTDEATH TX={} OBJECT={} END\\n", tx_id, object_id)
            }
        }
    }''',
     '''            WalEntry::ObjectDeath { tx_id, object_id } => {
                format!("OBJECTDEATH TX={} OBJECT={} END\\n", tx_id, object_id)
            }
            WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource } => {
                format!(
                    "CAPABILITYGRANT TX={} TYPE={} GRANTOR={} GRANTEE={} RESOURCE={} END\\n",
                    tx_id, cap_type, grantor, grantee, resource
                )
            }
        }
    }'''),

    ("deserialize 分派表加 CAPABILITYGRANT",
     '''            "OBJECTDEATH" => Self::deserialize_object_death(&parts),
            _ => None,
        }''',
     '''            "OBJECTDEATH" => Self::deserialize_object_death(&parts),
            "CAPABILITYGRANT" => Self::deserialize_capability_grant(&parts),
            _ => None,
        }'''),

    ("加 deserialize_capability_grant 函数",
     '''        Some(WalEntry::ObjectBirth { tx_id, object_id })
    }

    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {''',
     '''        Some(WalEntry::ObjectBirth { tx_id, object_id })
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
        Some(WalEntry::CapabilityGrant { tx_id, cap_type, grantor, grantee, resource })
    }

    fn deserialize_object_link(parts: &[&str]) -> Option<Self> {'''),
]

# ---------------- src/types.rs ----------------
types_edits = [
    ("加 PendingCapabilityGrant 结构体",
     '''pub struct TransactionContext {
    pub capabilities: Vec<u64>,''',
     '''#[derive(Debug, Clone)]
pub struct PendingCapabilityGrant {
    pub cap_type: String,
    pub grantor: ObjectId,
    pub grantee: ObjectId,
    pub resource: ObjectId,
}

pub struct TransactionContext {
    pub capabilities: Vec<u64>,'''),

    ("TransactionContext 加 pending_capabilities 字段",
     '''    pub pending_objects: Vec<ObjectId>,
    pub aborted: bool,''',
     '''    pub pending_objects: Vec<ObjectId>,
    pub pending_capabilities: Vec<PendingCapabilityGrant>,
    pub aborted: bool,'''),

    ("TransactionContext::new 初始化 pending_capabilities",
     '''            pending_objects: Vec::new(),
            pending_links: Vec::new(),''',
     '''            pending_objects: Vec::new(),
            pending_capabilities: Vec::new(),
            pending_links: Vec::new(),'''),
]

# ---------------- src/engine.rs ----------------
engine_edits = [
    ("object_birth 收窄可见性为 pub(crate)",
     '''    pub fn object_birth(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {''',
     '''    pub(crate) fn object_birth(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {'''),

    ("object_birth 内 AdminCap 改为暂存,不再立即写全局图",
     '''        ctx.pending_objects.push(object_id);

        // birth 时创建者自动获得该 Object 的 AdminCap
        let cap_id = {
            let mut cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.grant(
                "AdminCap".into(),
                object_id,
                object_id,
                object_id,
            )
        };
        ctx.capabilities.push(cap_id);

        Ok(())
    }''',
     '''        ctx.pending_objects.push(object_id);

        // birth 时创建者自动获得该 Object 的 AdminCap
        // P4.x: 不再立即写入 capability_graph(会在 abort 后残留、且 Recovery 时丢失)。
        // 改为暂存,Commit 时统一落地 + 写 WAL;Recovery 时统一重放。
        ctx.pending_capabilities.push(crate::types::PendingCapabilityGrant {
            cap_type: "AdminCap".into(),
            grantor: object_id,
            grantee: object_id,
            resource: object_id,
        });

        Ok(())
    }'''),

    ("commit() 里落地 pending_capabilities 并写 WAL",
     '''        // P4: 固化 Object 到全局注册表
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_objects {
                registry.insert(*object_id, crate::types::ObjectRecord::new_state(*object_id));
            }
        }

        // P8.1: OWNS 死亡闭包——from 死亡则 owned 对象一并进入 pending_deaths''',
     '''        // P4: 固化 Object 到全局注册表
        {
            let mut registry = self.object_registry.lock().unwrap();
            for object_id in &ctx.pending_objects {
                registry.insert(*object_id, crate::types::ObjectRecord::new_state(*object_id));
            }
        }

        // P4.x: 写入 CapabilityGrant WAL 条目并落地到 capability_graph
        // (Object 与其 Capability 视为同一语义闭合域,Commit 时一并生效)
        for grant in ctx.pending_capabilities.drain(..) {
            let grant_entry = WalEntry::CapabilityGrant {
                tx_id: ctx.tx_id(),
                cap_type: grant.cap_type.clone(),
                grantor: grant.grantor,
                grantee: grant.grantee,
                resource: grant.resource,
            };
            self.wal
                .append_and_sync(&grant_entry)
                .map_err(|e| VeritasError::EngineError(format!("WAL CapabilityGrant write failed: {}", e)))?;

            let mut cap_graph = self.capability_graph.lock().unwrap();
            cap_graph.grant(grant.cap_type, grant.grantor, grant.grantee, grant.resource);
        }

        // P8.1: OWNS 死亡闭包——from 死亡则 owned 对象一并进入 pending_deaths'''),

    ("recovery: 声明 recovered_cap_grants 收集容器",
     '''        let mut recovered_links: Vec<LinkEdge> = Vec::new();
        let mut recovered_deaths: Vec<ObjectId> = Vec::new();''',
     '''        let mut recovered_links: Vec<LinkEdge> = Vec::new();
        let mut recovered_deaths: Vec<ObjectId> = Vec::new();
        let mut recovered_cap_grants: Vec<(String, ObjectId, ObjectId, ObjectId)> = Vec::new();'''),

    ("recovery: WAL 循环里收集 CapabilityGrant",
     '''                WalEntry::ObjectLink { from, to, link_type, .. } => {
                    let relation = match link_type {
                        0 => LinkType::DependsOn,
                        1 => LinkType::Owns,
                        2 => LinkType::References,
                        _ => continue,
                    };
                    recovered_links.push(LinkEdge { from: *from, to: *to, link_type: relation });
                }
                _ => {}
            }
        }''',
     '''                WalEntry::ObjectLink { from, to, link_type, .. } => {
                    let relation = match link_type {
                        0 => LinkType::DependsOn,
                        1 => LinkType::Owns,
                        2 => LinkType::References,
                        _ => continue,
                    };
                    recovered_links.push(LinkEdge { from: *from, to: *to, link_type: relation });
                }
                WalEntry::CapabilityGrant { cap_type, grantor, grantee, resource, .. } => {
                    recovered_cap_grants.push((cap_type.clone(), *grantor, *grantee, *resource));
                }
                _ => {}
            }
        }'''),

    ("recovery: 真正重放 grant,不再只重放 revoke",
     '''        // P8-final: 重放能力级联撤销
        {
            let mut cap_graph = CapabilityGraph::new();
            for dead_obj in &recovered_deaths {
                cap_graph.revoke_holder(*dead_obj);
            }
        }''',
     '''        // P8-final: 重放 Capability Grant + 级联撤销
        // (原实现只重放了 revoke、从未重放 grant,导致重启后所有 Capability 丢失,这里一并修正)
        let recovered_cap_graph = {
            let mut cap_graph = CapabilityGraph::new();
            for (cap_type, grantor, grantee, resource) in &recovered_cap_grants {
                cap_graph.grant(cap_type.clone(), *grantor, *grantee, *resource);
            }
            for dead_obj in &recovered_deaths {
                cap_graph.revoke_holder(*dead_obj);
            }
            cap_graph
        };'''),

    ("recovery: 引擎构造时使用重放出的 cap_graph",
     '''            capability_graph: Mutex::new(CapabilityGraph::new()),''',
     '''            capability_graph: Mutex::new(recovered_cap_graph),'''),
]

ok = True
ok &= apply("src/wal.rs", wal_edits)
ok &= apply("src/types.rs", types_edits)
ok &= apply("src/engine.rs", engine_edits)

if ok:
    print("\n全部锚点匹配成功,已写入。接下来跑: cargo build 2>&1 | less")
else:
    print("\n有锚点没匹配上,对应文件已跳过未改动(备份文件已生成,原文件不受影响)。把上面 [FAIL] 的文件贴出来我再调整。")
