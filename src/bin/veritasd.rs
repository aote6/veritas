//! veritasd — JSON Lines World Interface daemon for system software (Forge).
//!
//! Protocol: one JSON object per line on stdin; one JSON response per line on stdout.
//! Does not expose KernelCall. All mutations go through WorldService → Kernel.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use serde_json::{json, Value};
use veritas_kernel::kernel::Kernel;
use veritas_kernel::types::ObjectState;
use veritas_kernel::world_api::{ReceiptView, WorldService};


fn root_hex(root: &[u8; 32]) -> String {
    hex::encode(root)
}

fn state_str(s: ObjectState) -> &'static str {
    match s {
        ObjectState::Alive => "Alive",
        ObjectState::Frozen => "Frozen",
        ObjectState::Dead => "Dead",
    }
}

fn link_type_str(lt: veritas_kernel::types::LinkType) -> &'static str {
    match lt {
        veritas_kernel::types::LinkType::DependsOn => "depends_on",
        veritas_kernel::types::LinkType::Owns => "owns",
        veritas_kernel::types::LinkType::References => "references",
    }
}

fn receipt_json(r: &ReceiptView) -> Value {
    let delta = &r.delta;
    let memory: Vec<Value> = delta
        .memory_written
        .iter()
        .map(|w| {
            json!({
                "object_id": w.object_id,
                "state_id": w.state_id,
                "value_hex": w.value_hex,
            })
        })
        .collect();
    let capability_grants: Vec<Value> = delta
        .capability_grants
        .iter()
        .map(|g| {
            json!({
                "capability_id": g.capability_id,
                "cap_type": g.cap_type,
                "grantor": g.grantor,
                "grantee": g.grantee,
                "resource": g.resource,
            })
        })
        .collect();
    json!({
        "tx_id": r.tx_id,
        "before_root": root_hex(&r.before_root),
        "after_root": root_hex(&r.after_root),
        "version": r.version,
        "delta": {
            "actor_id": delta.actor_id,
            "objects_created": delta.objects_created,
            "objects_deleted": delta.objects_deleted,
            "objects_frozen": delta.objects_frozen,
            "links_added": delta.links_added,
            "links_removed": delta.links_removed,
            "memory_written": memory,
            "capability_events": delta.capability_events,
            "capability_grants": capability_grants,
            "effects": delta.effects,
        }
    })
}

fn handle(world: &WorldService, req: &Value) -> Value {
    let cmd = req.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
    match cmd {
        "ping" => json!({"ok": true, "result": "pong"}),

        "world_info" => {
            let info = world.world_info();
            json!({
                "ok": true,
                "version": info.version,
                "state_root": root_hex(&info.state_root),
                "object_count": info.object_count,
            })
        }

        "list_objects" => {
            let objects: Vec<Value> = world
                .list_objects()
                .into_iter()
                .map(|o| json!({"id": o.id, "state": state_str(o.state)}))
                .collect();
            json!({"ok": true, "objects": objects})
        }

        "get_object" => {
            let id = match req.get("id").and_then(|v| v.as_u64()) {
                Some(id) => id,
                None => return json!({"ok": false, "error": "missing id"}),
            };
            match world.get_object(id) {
                Some(o) => json!({
                    "ok": true,
                    "object": {"id": o.id, "state": state_str(o.state)}
                }),
                None => json!({"ok": false, "error": "not found"}),
            }
        }

        "get_links" => {
            let links: Vec<Value> = world
                .list_links()
                .into_iter()
                .map(|l| {
                    json!({
                        "from": l.from,
                        "to": l.to,
                        "link_type": link_type_str(l.link_type),
                    })
                })
                .collect();
            json!({"ok": true, "links": links})
        }

        "attach_identity" => {
            let oid = req.get("object_id").and_then(|v| v.as_u64());
            match world.attach_identity(oid) {
                Ok(id) => json!({"ok": true, "object_id": id}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "whoami" => match world.whoami() {
            Some(id) => json!({"ok": true, "object_id": id}),
            None => json!({"ok": false, "error": "no identity attached"}),
        },

        // Legacy short-tx create (compat). Prefer tx_* session path.
        "create_object" => match world.create_object_short() {
            Ok((id, admin_cap)) => json!({
                "ok": true,
                "object": {"id": id, "state": "Alive", "admin_cap_id": admin_cap}
            }),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },

        "tx_begin" => {
            let actor = req.get("actor_id").and_then(|v| v.as_u64());
            match world.tx_begin(actor) {
                Ok(sid) => json!({"ok": true, "session_id": sid}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_create_object" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            match world.tx_create_object(sid) {
                Ok(id) => json!({"ok": true, "object_id": id}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_freeze_object" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let oid = match req.get("object_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing object_id"}),
            };
            match world.tx_freeze_object(sid, oid) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_death_object" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let oid = match req.get("object_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing object_id"}),
            };
            match world.tx_death_object(sid, oid) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_link" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let from = match req.get("from").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing from"}),
            };
            let to = match req.get("to").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing to"}),
            };
            let lt = req
                .get("link_type")
                .and_then(|v| v.as_str())
                .unwrap_or("owns");
            match world.tx_link(sid, from, to, lt) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_unlink" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let from = match req.get("from").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing from"}),
            };
            let to = match req.get("to").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing to"}),
            };
            match world.tx_unlink(sid, from, to) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_write" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let state_id = match req.get("state_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing state_id"}),
            };
            let payload = if let Some(h) = req.get("hex").and_then(|v| v.as_str()) {
                match hex::decode(h) {
                    Ok(b) => b,
                    Err(e) => return json!({"ok": false, "error": format!("bad hex: {}", e)}),
                }
            } else if let Some(s) = req.get("value").and_then(|v| v.as_str()) {
                s.as_bytes().to_vec()
            } else {
                return json!({"ok": false, "error": "missing hex or value"});
            };
            let object_id = req.get("object_id").and_then(|v| v.as_u64());
            match world.tx_write(sid, state_id, payload, object_id) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_read" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let state_id = match req.get("state_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing state_id"}),
            };
            match world.tx_read(sid, state_id) {
                Ok(bytes) => json!({
                    "ok": true,
                    "object_id": req.get("object_id"),
                    "state_id": state_id,
                    "value_hex": hex::encode(&bytes),
                }),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_commit" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            match world.tx_commit(sid) {
                Ok(r) => json!({"ok": true, "receipt": receipt_json(&r)}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_abort" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            match world.tx_abort(sid) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "tx_capability_grant" => {
            let sid = match req.get("session_id").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing session_id"}),
            };
            let grantor = match req.get("grantor").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing grantor"}),
            };
            let grantee = match req.get("grantee").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing grantee"}),
            };
            let resource = match req.get("resource").and_then(|v| v.as_u64()) {
                Some(s) => s,
                None => return json!({"ok": false, "error": "missing resource"}),
            };
            let capability_type = match req.get("capability_type").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return json!({"ok": false, "error": "missing capability_type"}),
            };
            match world.tx_capability_grant(sid, grantor, grantee, capability_type, resource) {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }

        "receipts_since" => {
            let version = req.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            let limit = req
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let receipts: Vec<Value> = world
                .receipts_since(version, limit)
                .iter()
                .map(|r| receipt_json(r))
                .collect();
            json!({"ok": true, "receipts": receipts})
        }
        "" => json!({"ok": false, "error": "missing cmd"}),
        other => json!({"ok": false, "error": format!("unknown cmd: {}", other)}),
    }
}

fn main() {
    let wal_path = std::env::var("VERITAS_WAL").ok();
    let kernel = if let Some(ref path) = wal_path {
        Arc::new(Kernel::with_wal_path(path.clone()))
    } else {
        Arc::new(Kernel::new())
    };
    let world = if let Some(path) = wal_path {
        WorldService::with_wal(kernel, path)
    } else {
        WorldService::new(kernel)
    };

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut lines = stdin.lock().lines();

    while let Some(Ok(line)) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    stdout,
                    "{}",
                    json!({"ok": false, "error": format!("bad json: {}", e)})
                );
                let _ = stdout.flush();
                continue;
            }
        };
        let resp = handle(&world, &req);
        let _ = writeln!(stdout, "{}", resp);
        let _ = stdout.flush();
    }
}
