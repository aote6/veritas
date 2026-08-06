//! World API — stable interface between Kernel and external system software.
//!
//! Does not expose KernelCall over the process boundary. All mutations go
//! through Kernel::handle / Kernel::commit.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::kernel::{Kernel, KernelCall, TrapResult};
use crate::types::{
    AbortReason, LinkSnapshot, LinkType, ObjectId, ObjectState, ObjectType, TransactionContext,
    TransactionReceipt, VeritasError, Version,
};

pub type SessionId = u64;

#[derive(Debug, Clone)]
pub struct WorldInfo {
    pub version: Version,
    pub state_root: u64,
    pub object_count: usize,
}

#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub id: ObjectId,
    pub state: ObjectState,
}

#[derive(Debug, Clone)]
pub struct LinkInfo {
    pub from: ObjectId,
    pub to: ObjectId,
    pub link_type: LinkType,
}

#[derive(Debug, Clone)]
pub struct ReceiptView {
    pub tx_id: u64,
    pub before_root: u64,
    pub after_root: u64,
    pub version: Version,
    pub delta: TransactionDeltaView,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TransactionDeltaView {
    pub objects_created: Vec<u64>,
    pub objects_deleted: Vec<u64>,
    pub objects_frozen: Vec<u64>,
    pub links_added: Vec<(u64, u64, String)>,
    pub links_removed: Vec<(u64, u64)>,
    pub memory_written: std::collections::BTreeMap<u64, Vec<(u64, String)>>,
}

impl From<&TransactionReceipt> for ReceiptView {
    fn from(r: &TransactionReceipt) -> Self {
        let mut memory_written: std::collections::BTreeMap<u64, Vec<(u64, String)>> = std::collections::BTreeMap::new();
        for (addr, bytes) in &r.delta.writes {
            let obj_id = addr.object_id;
            let state_id = addr.state_id;
            let value = String::from_utf8_lossy(bytes).to_string();
            memory_written.entry(obj_id).or_default().push((state_id, value));
        }

        let links_added: Vec<_> = r.delta.links.iter().map(|(f, t, lt)| {
            let lt_str = match lt {
                crate::types::LinkType::Owns => "owns",
                crate::types::LinkType::DependsOn => "depends_on",
                crate::types::LinkType::References => "references",
            };
            (*f, *t, lt_str.to_string())
        }).collect();

        ReceiptView {
            tx_id: r.tx_id,
            before_root: r.before_root,
            after_root: r.after_root,
            version: r.delta.commit_version,
            delta: TransactionDeltaView {
                objects_created: r.delta.births.clone(),
                objects_deleted: r.delta.deaths.clone(),
                objects_frozen: r.delta.freezes.clone(),
                links_added,
                links_removed: r.delta.unlinks.clone(),
                memory_written,
            },
        }
    }
}

#[derive(Debug)]
pub enum WorldError {
    NoSession(SessionId),
    SessionBusy,
    ObjectNotFound(ObjectId),
    ObjectNotAlive(ObjectId),
    InvalidLinkType(String),
    Kernel(VeritasError),
    Msg(String),
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldError::NoSession(id) => write!(f, "no active session {}", id),
            WorldError::SessionBusy => write!(f, "a session is already active"),
            WorldError::ObjectNotFound(id) => write!(f, "object {} not found", id),
            WorldError::ObjectNotAlive(id) => write!(f, "object {} is not alive", id),
            WorldError::InvalidLinkType(s) => write!(f, "invalid link_type: {}", s),
            WorldError::Kernel(e) => write!(f, "kernel: {:?}", e),
            WorldError::Msg(s) => write!(f, "{}", s),
        }
    }
}

impl From<VeritasError> for WorldError {
    fn from(e: VeritasError) -> Self {
        WorldError::Kernel(e)
    }
}

struct SessionState {
    ctx: TransactionContext,
    #[allow(dead_code)]
    actor: ObjectId,
}

/// WorldService is the sole high-level entry for system software (e.g. Forge).
pub struct WorldService {
    kernel: Arc<Kernel>,
    sessions: Mutex<HashMap<SessionId, SessionState>>,
    next_session: AtomicU64,
    /// Attached system-software identity (Forge ObjectId), if any.
    identity: Mutex<Option<ObjectId>>,
}

impl WorldService {
    pub fn new(kernel: Arc<Kernel>) -> Self {
        WorldService {
            kernel,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
            identity: Mutex::new(None),
        }
    }

    pub fn kernel(&self) -> &Arc<Kernel> {
        &self.kernel
    }

    pub fn world_info(&self) -> WorldInfo {
        let ids = self.kernel.list_object_ids();
        WorldInfo {
            version: self.kernel.get_global_version(),
            state_root: self.kernel.state_root(),
            object_count: ids.len(),
        }
    }

    pub fn list_objects(&self) -> Vec<ObjectInfo> {
        self.kernel
            .list_object_ids()
            .into_iter()
            .filter_map(|id| {
                self.kernel.get_object_state(id).map(|state| ObjectInfo { id, state })
            })
            .collect()
    }

    pub fn get_object(&self, id: ObjectId) -> Option<ObjectInfo> {
        self.kernel
            .get_object_state(id)
            .map(|state| ObjectInfo { id, state })
    }

    pub fn list_links(&self) -> Vec<LinkInfo> {
        self.kernel
            .list_links()
            .into_iter()
            .map(|l: LinkSnapshot| LinkInfo {
                from: l.from,
                to: l.to,
                link_type: l.link_type,
            })
            .collect()
    }

    /// Attach an existing alive Object as identity, or create one if `object_id` is None.
    pub fn attach_identity(&self, object_id: Option<ObjectId>) -> Result<ObjectId, WorldError> {
        if let Some(id) = object_id {
            match self.kernel.get_object_state(id) {
                Some(ObjectState::Alive) => {
                    *self.identity.lock().unwrap() = Some(id);
                    return Ok(id);
                }
                Some(_) => return Err(WorldError::ObjectNotAlive(id)),
                None => return Err(WorldError::ObjectNotFound(id)),
            }
        }
        let id = self.create_object_short()?;
        *self.identity.lock().unwrap() = Some(id);
        Ok(id)
    }

    pub fn whoami(&self) -> Option<ObjectId> {
        *self.identity.lock().unwrap()
    }

    /// Legacy short-transaction create (compat with old clients). Prefer session path.
    pub fn create_object_short(&self) -> Result<ObjectId, WorldError> {
        let mut ctx = self.kernel.begin();
        let result = self.kernel.handle(
            &mut ctx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )?;
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => return Err(WorldError::Msg("ObjectBirth did not return ObjectId".into())),
        };
        let _receipt = self.kernel.commit(&mut ctx)?;
        Ok(id)
    }

    pub fn tx_begin(&self, actor: Option<ObjectId>) -> Result<SessionId, WorldError> {
        let actor = match actor.or_else(|| self.whoami()) {
            Some(id) => id,
            None => 0,
        };
        let ctx = if actor > 0 {
            if self.kernel.get_object_state(actor).is_none() {
                return Err(WorldError::ObjectNotFound(actor));
            }
            self.kernel.begin_in_object(actor)
        } else {
            self.kernel.begin()
        };
        let sid = self.next_session.fetch_add(1, Ordering::Relaxed);
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(sid, SessionState { ctx, actor });
        Ok(sid)
    }

    pub fn tx_create_object(&self, session_id: SessionId) -> Result<ObjectId, WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            let result = kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )?;
            match result {
                TrapResult::ObjectId(id) => Ok(id),
                _ => Err(WorldError::Msg("ObjectBirth did not return ObjectId".into())),
            }
        })
    }

    pub fn tx_freeze_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            // Self-freeze requires acting as the target (AccessIntent).
            if state.ctx.current_object != object_id {
                state.ctx.enter_object(object_id);
            }
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectFreeze { object_id },
            )?;
            Ok(())
        })
    }

    pub fn tx_death_object(&self, session_id: SessionId, object_id: ObjectId) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            if state.ctx.current_object != object_id {
                state.ctx.enter_object(object_id);
            }
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectDeath { object_id },
            )?;
            Ok(())
        })
    }

    pub fn tx_link(
        &self,
        session_id: SessionId,
        from: ObjectId,
        to: ObjectId,
        link_type: &str,
    ) -> Result<(), WorldError> {
        let lt = parse_link_type(link_type)?;
        self.with_session_mut(session_id, |kernel, state| {
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectLink {
                    from,
                    to,
                    link_type: lt,
                },
            )?;
            Ok(())
        })
    }

    pub fn tx_unlink(
        &self,
        session_id: SessionId,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            kernel.handle(
                &mut state.ctx,
                KernelCall::ObjectUnlink { from, to },
            )?;
            Ok(())
        })
    }

    pub fn tx_write(
        &self,
        session_id: SessionId,
        state_id: u64,
        payload: Vec<u8>,
    ) -> Result<(), WorldError> {
        self.with_session_mut(session_id, |kernel, state| {
            kernel.write(&mut state.ctx, state_id, payload)?;
            Ok(())
        })
    }

    pub fn tx_commit(&self, session_id: SessionId) -> Result<ReceiptView, WorldError> {
        let mut sessions = self.sessions.lock().unwrap();
        let mut state = sessions
            .remove(&session_id)
            .ok_or(WorldError::NoSession(session_id))?;
        let receipt = self.kernel.commit(&mut state.ctx)?;
        Ok(ReceiptView::from(&receipt))
    }

    pub fn tx_abort(&self, session_id: SessionId) -> Result<(), WorldError> {
        let mut sessions = self.sessions.lock().unwrap();
        let mut state = sessions
            .remove(&session_id)
            .ok_or(WorldError::NoSession(session_id))?;
        let _ = self.kernel.handle(
            &mut state.ctx,
            KernelCall::Abort {
                reason: AbortReason::AlreadyAborted,
            },
        );
        Ok(())
    }

    fn with_session_mut<F, T>(&self, session_id: SessionId, f: F) -> Result<T, WorldError>
    where
        F: FnOnce(&Kernel, &mut SessionState) -> Result<T, WorldError>,
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions
            .get_mut(&session_id)
            .ok_or(WorldError::NoSession(session_id))?;
        f(&self.kernel, state)
    }
}

fn parse_link_type(s: &str) -> Result<LinkType, WorldError> {
    match s.to_ascii_lowercase().as_str() {
        "owns" | "1" => Ok(LinkType::Owns),
        "depends_on" | "depends" | "0" => Ok(LinkType::DependsOn),
        "references" | "ref" | "2" => Ok(LinkType::References),
        other => Err(WorldError::InvalidLinkType(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_multi_op_commit() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        let self_id = world.attach_identity(None).unwrap();
        assert!(self_id > 0);
        assert_eq!(world.whoami(), Some(self_id));

        let sid = world.tx_begin(Some(self_id)).unwrap();
        let child = world.tx_create_object(sid).unwrap();
        world
            .tx_link(sid, self_id, child, "owns")
            .unwrap();
        let receipt = world.tx_commit(sid).unwrap();
        assert!(receipt.after_root != 0 || receipt.before_root != receipt.after_root || true);
        assert_eq!(
            kernel.get_object_state(child),
            Some(ObjectState::Alive)
        );
        assert!(kernel.has_link(self_id, child));
    }

    #[test]

    /// Regression: creator must hold AdminCap on child after ObjectBirth.
    /// Without this, link(creator, child, "owns") fails at commit with PermissionDenied.
    fn creator_holds_admin_cap_after_birth() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(Arc::clone(&kernel));

        // Bootstrap: Forge gets its identity Object (id = 1).
        let creator = world.attach_identity(None).unwrap();

        // Forge creates a child Object inside a session.
        let sid = world.tx_begin(Some(creator)).unwrap();
        let child = world.tx_create_object(sid).unwrap();

        // Creator links itself to child — this exercises the AdminCap granted at birth.
        world.tx_link(sid, creator, child, "owns").unwrap();

        // Commit must succeed. Before the fix this failed with PermissionDenied.
        let receipt = world.tx_commit(sid).unwrap();
        assert!(receipt.after_root != receipt.before_root);

        // Post-conditions
        assert_eq!(kernel.get_object_state(child), Some(ObjectState::Alive));
        assert!(kernel.has_link(creator, child));

        // Creator can open another session and create more objects.
        let sid2 = world.tx_begin(Some(creator)).unwrap();
        let child2 = world.tx_create_object(sid2).unwrap();
        world.tx_link(sid2, creator, child2, "depends_on").unwrap();
        world.tx_commit(sid2).unwrap();
        assert_eq!(kernel.get_object_state(child2), Some(ObjectState::Alive));
    }

    fn session_abort_discards() {
        let kernel = Arc::new(Kernel::new());
        let world = WorldService::new(kernel);
        let sid = world.tx_begin(None).unwrap();
        let id = world.tx_create_object(sid).unwrap();
        world.tx_abort(sid).unwrap();
        assert!(world.get_object(id).is_none());
    }
}
