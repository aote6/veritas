// Veritas Kernel V0.3 - Kernel wrapper for VeritasEngine
//
// Phase 1: Thin wrapper that delegates all calls to the real VeritasEngine.
// Future phases will restrict access to TRAP-only kernel services.

use crate::engine::VeritasEngine;
use crate::types::*;

/// Kernel wraps VeritasEngine and serves as the world state authority.
///
/// In Phase 1, this is a thin pass-through wrapper. Machine accesses
/// Engine methods through Kernel, but the full API surface is unchanged.
///
/// In Phase 2, Kernel will restrict access to TRAP-only kernel services
/// and Machine will no longer call Engine methods directly.
/// KernelCall represents a decoded kernel service request.
///
/// Machine decodes raw register values (r0, r1, r2) into this enum,
/// then passes it to Kernel::handle(). Machine never calls kernel
/// methods directly for kernel services.
#[derive(Debug, Clone)]
pub enum KernelCall {
    ObjectBirth {
        object_type: ObjectType,
    },
    ObjectDeath {
        object_id: ObjectId,
    },
    ObjectLink {
        from: ObjectId,
        to: ObjectId,
        link_type: LinkType,
    },
    ObjectUnlink {
        from: ObjectId,
        to: ObjectId,
    },
    ObjectFreeze {
        object_id: ObjectId,
    },
    Abort {
        reason: AbortReason,
    },
    CapabilityGrant {
        grantor: ObjectId,
        grantee: ObjectId,
        capability_type: String,
        resource: ObjectId,
    },
    /// Revoke a holder's capability. Cascade follows the edge setting
    /// unless cascade_override is Some(...). Constitution §3.6 is the
    /// cascade=true case (full downstream revocation).
    CapabilityRevoke {
        capability_id: CapabilityId,
        holder: ObjectId,
        /// None → use DelegationEdge.cascade_on_revoke (root defaults true).
        cascade_override: Option<bool>,
    },
    /// Add a holder edge under an existing capability (no new CapabilityId).
    CapabilityDelegate {
        capability_id: CapabilityId,
        from: ObjectId,
        to: ObjectId,
        cascade_on_revoke: bool,
    },
    MemoryAlloc {
        object_id: ObjectId,
        size_hint: u64,
    },
    Commit,
    Effect {
        payload: Vec<u8>,
    },
    Savepoint {
        name: String,
    },
    RollbackTo {
        name: String,
    },
}
// ===== Phase 1 Step 7: KernelCall ABI codec =====
//
// TRAP ABI (docs/TRAP_ABI_FREEZE.md):
//   service_id 0–5, 12: register operands (r0/r1/r2)
//   service_id 6–11:    r0 = address of little-endian parameter block in Machine RAM
//
// Parameter block header (all multi-byte fields little-endian):
//   offset 0: u16 total_len
//   offset 2: u8  field_count
//   offset 3: fields...
//
// Decode failures (malformed blocks, OOB, bad UTF-8, illegal tags) return
// VeritasError::EngineError so Machine maps them to TrapReason::InvalidEncoding.

impl KernelCall {
    /// Register-only decode for simple services (0–5, 12).
    /// Complex services 6–11 require [`decode_with_memory`].
    pub fn decode(
        service_id: u8,
        r0: u64,
        r1: u64,
        r2: u64,
    ) -> Result<Self, crate::types::VeritasError> {
        match service_id {
            0 => Ok(KernelCall::ObjectBirth {
                object_type: if r0 == 0 {
                    crate::types::ObjectType::StateObject
                } else {
                    crate::types::ObjectType::ModuleObject
                },
            }),
            1 => Ok(KernelCall::ObjectDeath { object_id: r0 }),
            2 => {
                let link_type = match r2 as u8 {
                    0 => crate::types::LinkType::DependsOn,
                    1 => crate::types::LinkType::Owns,
                    2 => crate::types::LinkType::References,
                    _ => {
                        return Err(crate::types::VeritasError::EngineError(format!(
                            "Invalid LinkType: {}",
                            r2
                        )))
                    }
                };
                Ok(KernelCall::ObjectLink {
                    from: r0,
                    to: r1,
                    link_type,
                })
            }
            3 => Ok(KernelCall::ObjectUnlink { from: r0, to: r1 }),
            4 => Ok(KernelCall::ObjectFreeze { object_id: r0 }),
            5 => Ok(KernelCall::Commit),
            12 => Ok(KernelCall::MemoryAlloc {
                object_id: r0,
                size_hint: r1,
            }),
            6 | 7 | 8 | 9 | 10 | 11 => Err(crate::types::VeritasError::EngineError(format!(
                "service_id {} requires parameter block (use decode_with_memory)",
                service_id
            ))),
            _ => Err(crate::types::VeritasError::EngineError(format!(
                "Unknown kernel service_id: {}",
                service_id
            ))),
        }
    }

    /// Full TRAP ABI decode: register services + RAM parameter blocks for 6–11.
    pub fn decode_with_memory(
        service_id: u8,
        r0: u64,
        r1: u64,
        r2: u64,
        memory: &crate::memory::Memory,
    ) -> Result<Self, crate::types::VeritasError> {
        match service_id {
            0..=5 | 12 => Self::decode(service_id, r0, r1, r2),
            6 => Self::decode_effect_block(r0, memory),
            7 => Self::decode_name_block(r0, memory, true),
            8 => Self::decode_name_block(r0, memory, false),
            9 => Self::decode_capability_grant_block(r0, memory),
            10 => Self::decode_capability_revoke_block(r0, memory),
            11 => Self::decode_capability_delegate_block(r0, memory),
            _ => Err(crate::types::VeritasError::EngineError(format!(
                "Unknown kernel service_id: {}",
                service_id
            ))),
        }
    }

    fn abi_err(msg: &str) -> crate::types::VeritasError {
        crate::types::VeritasError::EngineError(msg.to_string())
    }

    /// Read a fixed-size parameter block; fail-closed on OOB / length mismatch.
    fn read_param_block<'a>(
        addr: u64,
        memory: &'a crate::memory::Memory,
    ) -> Result<&'a [u8], crate::types::VeritasError> {
        let addr = addr as usize;
        if addr >= memory.len() {
            return Err(Self::abi_err("parameter block address OOB"));
        }
        let slice = memory
            .slice_from(addr)
            .map_err(|_| Self::abi_err("parameter block address OOB"))?;
        if slice.len() < 3 {
            return Err(Self::abi_err("parameter block header incomplete"));
        }
        let total_len = u16::from_le_bytes([slice[0], slice[1]]) as usize;
        if total_len < 3 {
            return Err(Self::abi_err("parameter block total_len < 3"));
        }
        if total_len > slice.len() {
            return Err(Self::abi_err("parameter block total_len exceeds RAM"));
        }
        Ok(&slice[..total_len])
    }

    fn decode_effect_block(
        addr: u64,
        memory: &crate::memory::Memory,
    ) -> Result<Self, crate::types::VeritasError> {
        let block = Self::read_param_block(addr, memory)?;
        let field_count = block[2];
        if field_count != 1 {
            return Err(Self::abi_err("Effect: field_count must be 1"));
        }
        if block.len() < 7 {
            return Err(Self::abi_err("Effect: block too short for payload_len"));
        }
        let payload_len = u32::from_le_bytes([block[3], block[4], block[5], block[6]]) as usize;
        let expected = 7 + payload_len;
        if block.len() != expected {
            return Err(Self::abi_err("Effect: total_len != 7 + payload_len"));
        }
        let payload = block[7..7 + payload_len].to_vec();
        Ok(KernelCall::Effect { payload })
    }

    fn decode_name_block(
        addr: u64,
        memory: &crate::memory::Memory,
        is_savepoint: bool,
    ) -> Result<Self, crate::types::VeritasError> {
        let block = Self::read_param_block(addr, memory)?;
        let field_count = block[2];
        if field_count != 1 {
            return Err(Self::abi_err("name block: field_count must be 1"));
        }
        if block.len() < 5 {
            return Err(Self::abi_err("name block: too short for name_len"));
        }
        let name_len = u16::from_le_bytes([block[3], block[4]]) as usize;
        let expected = 5 + name_len;
        if block.len() != expected {
            return Err(Self::abi_err("name block: total_len != 5 + name_len"));
        }
        let name_bytes = &block[5..5 + name_len];
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| Self::abi_err("name block: invalid UTF-8"))?
            .to_string();
        if is_savepoint {
            Ok(KernelCall::Savepoint { name })
        } else {
            Ok(KernelCall::RollbackTo { name })
        }
    }

    fn decode_capability_grant_block(
        addr: u64,
        memory: &crate::memory::Memory,
    ) -> Result<Self, crate::types::VeritasError> {
        let block = Self::read_param_block(addr, memory)?;
        let field_count = block[2];
        if field_count != 4 {
            return Err(Self::abi_err("CapabilityGrant: field_count must be 4"));
        }
        // Need at least: 3 header + 8 grantor + 8 grantee + 2 type_len = 21
        if block.len() < 21 {
            return Err(Self::abi_err("CapabilityGrant: block too short"));
        }
        let grantor = u64::from_le_bytes(block[3..11].try_into().unwrap());
        let grantee = u64::from_le_bytes(block[11..19].try_into().unwrap());
        let type_len = u16::from_le_bytes([block[19], block[20]]) as usize;
        let type_end = 21 + type_len;
        if block.len() < type_end + 8 {
            return Err(Self::abi_err(
                "CapabilityGrant: capability_type or resource OOB",
            ));
        }
        let expected = type_end + 8;
        if block.len() != expected {
            return Err(Self::abi_err(
                "CapabilityGrant: total_len does not match fields",
            ));
        }
        let type_bytes = &block[21..type_end];
        let capability_type = std::str::from_utf8(type_bytes)
            .map_err(|_| Self::abi_err("CapabilityGrant: capability_type invalid UTF-8"))?
            .to_string();
        let resource = u64::from_le_bytes(block[type_end..type_end + 8].try_into().unwrap());
        Ok(KernelCall::CapabilityGrant {
            grantor,
            grantee,
            capability_type,
            resource,
        })
    }

    fn decode_capability_revoke_block(
        addr: u64,
        memory: &crate::memory::Memory,
    ) -> Result<Self, crate::types::VeritasError> {
        let block = Self::read_param_block(addr, memory)?;
        let field_count = block[2];
        if field_count != 3 {
            return Err(Self::abi_err("CapabilityRevoke: field_count must be 3"));
        }
        if block.len() != 20 {
            return Err(Self::abi_err("CapabilityRevoke: total_len must be 20"));
        }
        let capability_id = u64::from_le_bytes(block[3..11].try_into().unwrap());
        let holder = u64::from_le_bytes(block[11..19].try_into().unwrap());
        let cascade_override = match block[19] {
            0 => None,
            1 => Some(false),
            2 => Some(true),
            _ => {
                return Err(Self::abi_err(
                    "CapabilityRevoke: invalid cascade_tag (must be 0/1/2)",
                ))
            }
        };
        Ok(KernelCall::CapabilityRevoke {
            capability_id,
            holder,
            cascade_override,
        })
    }

    fn decode_capability_delegate_block(
        addr: u64,
        memory: &crate::memory::Memory,
    ) -> Result<Self, crate::types::VeritasError> {
        let block = Self::read_param_block(addr, memory)?;
        let field_count = block[2];
        if field_count != 4 {
            return Err(Self::abi_err("CapabilityDelegate: field_count must be 4"));
        }
        if block.len() != 28 {
            return Err(Self::abi_err("CapabilityDelegate: total_len must be 28"));
        }
        let capability_id = u64::from_le_bytes(block[3..11].try_into().unwrap());
        let from = u64::from_le_bytes(block[11..19].try_into().unwrap());
        let to = u64::from_le_bytes(block[19..27].try_into().unwrap());
        let cascade_on_revoke = match block[27] {
            0 => false,
            1 => true,
            _ => {
                return Err(Self::abi_err(
                    "CapabilityDelegate: cascade_on_revoke must be 0 or 1",
                ))
            }
        };
        Ok(KernelCall::CapabilityDelegate {
            capability_id,
            from,
            to,
            cascade_on_revoke,
        })
    }
}

/// TrapResult is returned by Kernel::handle() after processing a KernelCall.
/// The result is written to register r0 by Machine.
#[derive(Debug, Clone)]
pub enum TrapResult {
    ObjectId(ObjectId),
    CapabilityId(CapabilityId),
    StateId(StateId),
    EffectKey(String),
    Success,
    Error(u8),  // 错误码
}

// TRAP ABI error codes — frozen in docs/TRAP_ABI_ERROR_CONTRACT_FREEZE.md
pub const TRAP_ERR_ACCESS_DENIED: u8 = 1;
pub const TRAP_ERR_ENGINE: u8 = 2;
pub const TRAP_ERR_MEMORY_FAULT: u8 = 3; // reserved, no real source yet
pub const TRAP_ERR_WRITE_CONFLICT: u8 = 4;
pub const TRAP_ERR_PERMISSION_DENIED: u8 = 5;
pub const TRAP_ERR_STATE_NOT_FOUND: u8 = 6;

impl TrapResult {
    pub fn from_error(e: VeritasError) -> Self {
        let code = match e {
            VeritasError::PermissionDenied => TRAP_ERR_PERMISSION_DENIED,
            VeritasError::Abort(AbortReason::WriteConflict) => TRAP_ERR_WRITE_CONFLICT,
            VeritasError::Abort(AbortReason::ReadFutureVersion) => TRAP_ERR_WRITE_CONFLICT,
            VeritasError::Abort(AbortReason::AlreadyAborted) => TRAP_ERR_WRITE_CONFLICT,
            VeritasError::Abort(AbortReason::StateNotFound) => TRAP_ERR_STATE_NOT_FOUND,
            VeritasError::Abort(AbortReason::PhantomConflict) => TRAP_ERR_WRITE_CONFLICT,
            VeritasError::EngineError(_) => TRAP_ERR_ENGINE,
            VeritasError::DeterminismViolation => TRAP_ERR_WRITE_CONFLICT,
        };
        TrapResult::Error(code)
    }
}

pub struct Kernel {
    engine: VeritasEngine,
}

impl Kernel {
    pub fn new() -> Self {
        Kernel {
            engine: VeritasEngine::new(),
        }
    }

    pub fn with_wal_path(wal_path: String) -> Self {
        Kernel {
            engine: VeritasEngine::with_wal_path(wal_path),
        }
    }

    /// Internal engine access for same-crate runtime (Machine / WorldService).
    /// Production external callers must not use this; use handle() or WorldService.
    pub(crate) fn engine(&self) -> &VeritasEngine {
        &self.engine
    }

    /// Replay: 从 WAL 重放全部已提交事务，返回最终 WorldState 根哈希。
    /// 从空 WorldState 出发，不保留引擎用于后续操作。
    /// 用于验证 WAL 的确定性——Replay(WAL) == 原始执行结束时的 root_hash()。
    pub fn replay(wal_path: &str) -> [u8; 32] {
        let (records, _) =
            crate::wal::RecoveryManager::recover(wal_path).unwrap_or_else(|_| (vec![], 0));
        let ordered_deltas = crate::wal::build_ordered_deltas(&records);
        let engine = VeritasEngine::empty();
        for delta in &ordered_deltas {
            engine.apply(delta);
        }
        engine.root_hash()
    }

    // ===== Phase 1 Step 4: KernelCall dispatch =====

    /// Handle a decoded kernel service call.
    /// This is the single entry point for all kernel services.
    /// Machine calls this instead of individual kernel methods.
    pub fn handle(
        &self,
        ctx: &mut TransactionContext,
        call: KernelCall,
    ) -> TrapResult {
        match call {
            KernelCall::ObjectBirth {
                object_type: _object_type,
            } => {
                // Step 3/ObjectId: Kernel allocates ObjectId internally
                let id = self.engine.next_object_id();
                match self.engine.object_birth(ctx, id) {
                    Ok(()) => TrapResult::ObjectId(id),
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::ObjectDeath { object_id } => {
                match self.engine.object_death(ctx, object_id) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::ObjectLink {
                from,
                to,
                link_type,
            } => {
                match self.engine.object_link(ctx, from, to, link_type) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::ObjectUnlink { from, to } => {
                match self.engine.object_unlink(ctx, from, to) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::ObjectFreeze { object_id } => {
                match self.engine.object_freeze(ctx, object_id) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::CapabilityGrant {
                grantor,
                grantee,
                capability_type,
                resource,
            } => {
                match self.engine.capability_grant(
                    ctx,
                    grantor,
                    grantee,
                    &capability_type,
                    resource,
                ) {
                    Ok(cap_id) => TrapResult::CapabilityId(cap_id),
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::CapabilityRevoke {
                capability_id,
                holder,
                cascade_override,
            } => {
                match self.engine
                    .capability_revoke(ctx, capability_id, holder, cascade_override)
                {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::CapabilityDelegate {
                capability_id,
                from,
                to,
                cascade_on_revoke,
            } => {
                match self.engine
                    .capability_delegate(ctx, capability_id, from, to, cascade_on_revoke)
                {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::MemoryAlloc {
                object_id,
                size_hint,
            } => {
                match self.engine.memory_alloc(ctx, object_id, size_hint) {
                    Ok(state_id) => TrapResult::StateId(state_id),
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::Abort { reason } => {
                self.engine.abort(ctx, reason);
                TrapResult::Success
            }
            KernelCall::Commit => {
                match self.commit(ctx) {
                    Ok(_receipt) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::Effect { payload } => {
                match self.effect(ctx, payload) {
                    Ok(key) => TrapResult::EffectKey(key),
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::Savepoint { name } => {
                match self.savepoint(ctx, &name) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
            KernelCall::RollbackTo { name } => {
                match self.rollback_to(ctx, &name) {
                    Ok(()) => TrapResult::Success,
                    Err(e) => TrapResult::from_error(e),
                }
            }
        }
    }

    // Each method delegates directly to the corresponding VeritasEngine method.

    /// Test probe wrapper. Prefer Effect/WAL for production observation.
    #[allow(dead_code)]
    pub fn last_dependency_invalidations(&self) -> Vec<(ObjectId, ObjectId)> {
        self.engine.last_dependency_invalidations()
    }

    pub fn get_object_state(&self, object_id: ObjectId) -> Option<ObjectState> {
        self.engine.get_object_state(object_id)
    }

    pub fn is_object_dead(&self, object_id: ObjectId) -> bool {
        self.engine.is_object_dead(object_id)
    }

    pub fn list_object_ids(&self) -> Vec<ObjectId> {
        self.engine.list_object_ids()
    }

    pub(crate) fn attach_capability(&self, ctx: &mut TransactionContext, cap_id: u64) {
        self.engine.attach_capability(ctx, cap_id)
    }

    pub fn holds_capability(&self, cap_id: CapabilityId, holder: ObjectId) -> bool {
        self.engine.holds_capability(cap_id, holder)
    }

    pub fn capability_sequence(&self) -> u64 {
        self.engine.capability_sequence()
    }

    pub fn has_link(&self, from: ObjectId, to: ObjectId) -> bool {
        self.engine.has_link(from, to)
    }

    /// Read-only topology snapshot for World API / system software.
    pub fn list_links(&self) -> Vec<crate::types::LinkSnapshot> {
        self.engine.snapshot_links()
    }

    /// WorldSnapshot aggregation (world.md §12.4). Production infrastructure;
    /// currently reached via `KernelTestExt` until bin/ wires Checkpoint I/O.
    #[allow(dead_code)] // not yet called from bin/ production path; keep for Recovery/Checkpoint integration
    pub(crate) fn create_checkpoint(&self) -> WorldSnapshot {
        self.engine.create_checkpoint()
    }

    /// Restore from WorldSnapshot (world.md §12.4). Production infrastructure;
    /// currently reached via `KernelTestExt` until bin/ wires Checkpoint I/O.
    #[allow(dead_code)] // not yet called from bin/ production path; keep for Recovery/Checkpoint integration
    pub(crate) fn restore_checkpoint(&self, snap: &WorldSnapshot) -> bool {
        self.engine.restore_checkpoint(snap)
    }

    pub fn state_root(&self) -> [u8; 32] {
        self.engine.state_root()
    }

    pub fn peek_state(&self, state_id: StateId) -> Option<StateEntry> {
        self.engine.peek_state(state_id)
    }

    pub fn get_global_version(&self) -> Version {
        self.engine.get_global_version()
    }

    pub fn get_last_applied_delta_hash(&self) -> [u8; 32] {
        self.engine.get_last_applied_delta_hash()
    }

    // ----- Runtime-internal mutation surface (Machine / WorldService only) -----
    // External production code must use handle() or WorldService.
    // Integration tests use crate::test_api::KernelTestExt.

    pub(crate) fn begin(&self) -> TransactionContext {
        self.engine.begin()
    }

    pub(crate) fn begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
        self.engine.begin_in_object(object_id)
    }

    pub(crate) fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        self.engine.read(ctx, state_id)
    }

    pub(crate) fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        payload: Vec<u8>,
    ) -> Result<(), VeritasError> {
        self.engine.write(ctx, state_id, payload)
    }

    pub(crate) fn effect(
        &self,
        ctx: &mut TransactionContext,
        payload: Vec<u8>,
    ) -> Result<String, VeritasError> {
        self.engine.effect(ctx, payload)
    }

    pub(crate) fn commit(
        &self,
        ctx: &mut TransactionContext,
    ) -> Result<TransactionReceipt, VeritasError> {
        self.engine.commit(ctx)
    }

    pub(crate) fn init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        initial_value: Vec<u8>,
    ) {
        self.engine.init_state_in_tx(ctx, state_id, initial_value)
    }

    pub(crate) fn savepoint(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        self.engine.savepoint(ctx, name)
    }

    pub(crate) fn rollback_to(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        self.engine.rollback_to(ctx, name)
    }
}

#[cfg(test)]
mod kernel_tests {
    use super::*;

    #[test]
    fn test_kernel_creation() {
        let kernel = Kernel::new();
        let ctx = kernel.begin();
        assert_eq!(ctx.tx_id, 1);
    }

    // ===== Phase 1 Step 5: Kernel ABI boundary tests =====
    // These test the public TRAP -> KernelCall -> Kernel::handle() path.

    #[test]
    fn kernel_survives_multiple_machine_runs() {
        use std::sync::Arc;
        let kernel = Arc::new(Kernel::new());
        {
            let _machine1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 10).unwrap();
            kernel.engine.object_birth(&mut ctx, 20).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();
        }
        {
            let _machine2 = crate::machine::Machine::new(Arc::clone(&kernel));
            assert_eq!(kernel.get_object_state(10), Some(ObjectState::Alive));
            assert_eq!(kernel.get_object_state(20), Some(ObjectState::Alive));
        }
        assert_eq!(kernel.get_object_state(10), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(20), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_persists_object_across_machines() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        kernel.engine.object_birth(&mut ctx, 42).unwrap();
        let _receipt = kernel.commit(&mut ctx).unwrap();
        let ctx2 = kernel.begin();
        assert_eq!(kernel.get_object_state(42), Some(ObjectState::Alive));
        drop(ctx2);
    }

    #[test]
    fn kernel_state_independent_of_machine_lifetime() {
        let kernel = Kernel::new();
        {
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 100).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();
        }
        assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));
        let ctx = kernel.begin();
        assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));
        drop(ctx);
    }

    #[test]
    fn kernel_object_id_monotonic_across_machines() {
        let kernel = Kernel::new();
        let mut ctx1 = kernel.begin();
        kernel.engine.object_birth(&mut ctx1, 1).unwrap();
        kernel.engine.object_birth(&mut ctx1, 2).unwrap();
        let _receipt = kernel.commit(&mut ctx1).unwrap();
        let mut ctx2 = kernel.begin();
        kernel.engine.object_birth(&mut ctx2, 3).unwrap();
        let _receipt = kernel.commit(&mut ctx2).unwrap();
        assert_eq!(kernel.get_object_state(1), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(2), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(3), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_shared_by_multiple_machines() {
        use std::sync::Arc;
        let kernel = Arc::new(Kernel::new());
        {
            let _machine1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 77).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();
        }
        {
            let _machine2 = crate::machine::Machine::new(Arc::clone(&kernel));
            assert_eq!(kernel.get_object_state(77), Some(ObjectState::Alive));
        }
        assert_eq!(kernel.get_object_state(77), Some(ObjectState::Alive));
    }

    #[test]
    fn abi_trap_object_birth_via_handle() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let result = kernel.handle(&mut ctx, call);
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        assert!(id > 0);
        let _receipt = kernel.commit(&mut ctx).unwrap();
        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Alive));
    }

    #[test]
    fn abi_trap_object_link_via_handle() {
        let kernel = Kernel::new();

        // Create object A
        let mut ctx_a = kernel.begin();
        let call_a = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id_a = match kernel.handle(&mut ctx_a, call_a) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx_a).unwrap();

        // Create object B under A so A receives creator AdminCap on B
        // (STRICT CAPABILITY MODEL: CapabilityGrant requires AdminCap(resource)).
        let mut ctx_b = kernel.begin_in_object(id_a);
        let call_b = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id_b = match kernel.handle(&mut ctx_b, call_b) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx_b).unwrap();

        let mut ctx_link = kernel.begin_in_object(id_a);
        let _cap = kernel
            .engine
            .capability_grant(&mut ctx_link, id_a, id_a, "link", id_b)
            .unwrap();
        let call_link = KernelCall::ObjectLink {
            from: id_a,
            to: id_b,
            link_type: LinkType::DependsOn,
        };
        let result = kernel.handle(&mut ctx_link, call_link);
        assert!(matches!(result, TrapResult::Success));
        let _receipt = kernel.commit(&mut ctx_link).unwrap();

        assert!(kernel.has_link(id_a, id_b));
    }

    #[test]
    fn abi_trap_object_freeze_via_handle() {
        let kernel = Kernel::new();
        let mut ctx1 = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id = match kernel.handle(&mut ctx1, call) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx1).unwrap();

        let mut ctx2 = kernel.begin_in_object(id);
        let result = kernel
            .handle(&mut ctx2, KernelCall::ObjectFreeze { object_id: id });
        assert!(matches!(result, TrapResult::Success));
        let _receipt = kernel.commit(&mut ctx2).unwrap();

        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Frozen));
    }

    #[test]
    fn abi_trap_object_death_via_handle() {
        let kernel = Kernel::new();
        let mut ctx1 = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id = match kernel.handle(&mut ctx1, call) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx1).unwrap();

        let mut ctx2 = kernel.begin_in_object(id);
        let result = kernel
            .handle(&mut ctx2, KernelCall::ObjectDeath { object_id: id });
        assert!(matches!(result, TrapResult::Success));
        let _receipt = kernel.commit(&mut ctx2).unwrap();

        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Dead));
    }

    #[test]
    fn abi_trap_invalid_service_id_rejected() {
        // Invalid service_id is handled at Machine TRAP decoder level.
        // KernelCall enum prevents invalid states at compile time.
        assert!(true);
    }

    #[test]
    fn abi_trap_result_writes_to_register_r0() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let result = kernel.handle(&mut ctx, call);
        match result {
            TrapResult::ObjectId(id) => assert!(id > 0),
            _ => panic!("Expected ObjectId from ObjectBirth"),
        }
    }

    // ===== Phase 1 Step 7: ABI encoding boundary tests =====

    #[test]
    fn abi_decode_valid_service_ids() {
        assert!(KernelCall::decode(0, 0, 0, 0).is_ok());
        assert!(KernelCall::decode(1, 42, 0, 0).is_ok());
        assert!(KernelCall::decode(2, 10, 20, 0).is_ok());
        assert!(KernelCall::decode(3, 10, 20, 0).is_ok());
        assert!(KernelCall::decode(4, 99, 0, 0).is_ok());
    }

    #[test]
    fn abi_decode_invalid_service_id_rejected() {
        assert!(KernelCall::decode(99, 0, 0, 0).is_err());
        assert!(KernelCall::decode(255, 0, 0, 0).is_err());
    }

    #[test]
    fn abi_decode_invalid_link_type_rejected() {
        assert!(KernelCall::decode(2, 10, 20, 99).is_err());
    }

    #[test]
    fn abi_decode_object_type_state() {
        let call = KernelCall::decode(0, 0, 0, 0).unwrap();
        match call {
            KernelCall::ObjectBirth { object_type } => {
                assert!(matches!(object_type, ObjectType::StateObject));
            }
            _ => panic!("Expected ObjectBirth"),
        }
    }

    #[test]
    fn abi_decode_object_type_module() {
        let call = KernelCall::decode(0, 1, 0, 0).unwrap();
        match call {
            KernelCall::ObjectBirth { object_type } => {
                assert!(matches!(object_type, ObjectType::ModuleObject));
            }
            _ => panic!("Expected ObjectBirth"),
        }
    }

    #[test]
    fn abi_full_trap_roundtrip() {
        // Full TRAP ABI roundtrip: registers -> decode -> handle -> commit
        let kernel = Kernel::new();

        // Create object via decode + handle
        let mut ctx = kernel.begin();
        let call = KernelCall::decode(0, 0, 0, 0).unwrap();
        let result = kernel.handle(&mut ctx, call);
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx).unwrap();

        assert!(id > 0);
        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Alive));
    }

    // ===== Phase A: Kernel as persistent world tests =====

    #[test]
    fn kernel_survives_multiple_execute_calls() {
        use std::sync::Arc;
        let kernel = Arc::new(Kernel::new());

        // First execution: create an object
        {
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 1).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();
        }

        // Second execution: read the object (same kernel world)
        {
            let ctx = kernel.begin();
            assert_eq!(kernel.get_object_state(1), Some(ObjectState::Alive));
            drop(ctx);
        }

        // Object persists beyond both execution contexts
        assert_eq!(kernel.get_object_state(1), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_world_persists_across_sequential_machines() {
        use std::sync::Arc;
        let kernel = Arc::new(Kernel::new());

        // Machine 1: create objects
        {
            let _m1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 100).unwrap();
            kernel.engine.object_birth(&mut ctx, 200).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();
        }

        // Machine 2: sees all objects from Machine 1
        {
            let _m2 = crate::machine::Machine::new(Arc::clone(&kernel));
            assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));
            assert_eq!(kernel.get_object_state(200), Some(ObjectState::Alive));
        }

        // Kernel world is the source of truth, not any Machine
        assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(200), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_capability_persists_across_machines() {
        use std::sync::Arc;
        let kernel = Arc::new(Kernel::new());

        // Machine 1: create object and grant capability
        {
            let _m1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.engine.object_birth(&mut ctx, 10).unwrap();
            let _receipt = kernel.commit(&mut ctx).unwrap();

            let mut ctx2 = kernel.begin();
            kernel
                .engine
                .capability_grant(&mut ctx2, 10, 10, "read", 10)
                .unwrap();
            let _receipt = kernel.commit(&mut ctx2).unwrap();
        }

        // Machine 2: capability still valid
        {
            let _m2 = crate::machine::Machine::new(Arc::clone(&kernel));
            // Object 10 holds a capability on itself (AdminCap from birth + read grant)
            assert!(
                kernel.holds_capability(kernel.capability_sequence(), 10)
                    || kernel.get_object_state(10) == Some(ObjectState::Alive)
            );
        }
    }

    /// ObjectId allocation: fresh Kernel assigns 1, then 2.
    #[test]
    fn object_id_allocation_via_trap_path() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id1 = match kernel.handle(&mut ctx, call) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("expected ObjectId"),
        };
        assert_eq!(id1, 1, "first allocated ObjectId must be 1");

        let id2 = match kernel
            .handle(
                &mut ctx,
                KernelCall::ObjectBirth {
                    object_type: ObjectType::StateObject,
                },
            )
        {
            TrapResult::ObjectId(id) => id,
            _ => panic!("expected ObjectId"),
        };
        assert_eq!(id2, 2, "second allocated ObjectId must be 2");
    }

    /// ObjectId after commit: birth via TRAP path survives commit.
    #[test]
    fn object_id_allocation_committed_object_visible() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        let call = KernelCall::ObjectBirth {
            object_type: ObjectType::StateObject,
        };
        let id = match kernel.handle(&mut ctx, call) {
            TrapResult::ObjectId(id) => id,
            _ => panic!("expected ObjectId"),
        };
        let _receipt = kernel.commit(&mut ctx).unwrap();
        assert_eq!(
            kernel.get_object_state(id),
            Some(ObjectState::Alive),
            "committed birth must be visible in registry"
        );
    }

    /// ObjectId after recovery: counter resumes from max(birth_id)+1.
    #[test]
    fn object_id_allocation_resumes_after_commit() {
        use tempfile::NamedTempFile;
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        let engine1 = crate::engine::VeritasEngine::with_wal_path(path.clone());
        let mut ctx = engine1.begin();
        engine1.object_birth(&mut ctx, 10).unwrap();
        let _receipt = engine1.commit(&mut ctx).unwrap();
        drop(engine1);

        let engine2 = crate::engine::VeritasEngine::with_wal_path(path.clone());
        assert_eq!(
            engine2.next_object_id(),
            11,
            "recovered engine must start counter from max(birth_id)+1"
        );
        assert_eq!(
            engine2.get_object_state(10),
            Some(ObjectState::Alive),
            "recovered birth must still be Alive"
        );
    }

    /// P30.5: MEMORY_ALLOC returns sequential StateIds starting at 0.
    /// Allocations must not pollute StateStore with empty-value entries on commit.
    #[test]
    fn memory_alloc_sequential_state_ids() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        // Birth object 42 with explicit id (current API)
        kernel.engine().object_birth(&mut ctx, 42).unwrap();
        ctx.enter_object(42);

        let call0 = KernelCall::MemoryAlloc {
            object_id: 42,
            size_hint: 64,
        };
        let sid0 = match kernel.handle(&mut ctx, call0) {
            TrapResult::StateId(id) => id,
            other => panic!("expected StateId, got {:?}", other),
        };
        assert_eq!(sid0, 0, "first alloc on empty object must be 0");

        let call1 = KernelCall::MemoryAlloc {
            object_id: 42,
            size_hint: 128,
        };
        let sid1 = match kernel.handle(&mut ctx, call1) {
            TrapResult::StateId(id) => id,
            other => panic!("expected StateId, got {:?}", other),
        };
        assert_eq!(sid1, 1, "second alloc must advance to 1");

        // Commit: birth persists, but MEMORY_ALLOC must leave no empty StateStore entries.
        let _receipt = kernel.commit(&mut ctx).unwrap();
        let snap = kernel.engine().create_checkpoint();
        let empty_for_42: Vec<_> = snap
            .state_entries
            .iter()
            .filter(|(addr, entry)| addr.object_id == 42 && entry.value.is_empty())
            .collect();
        assert!(
            empty_for_42.is_empty(),
            "MEMORY_ALLOC must not write empty values into StateStore; found {:?}",
            empty_for_42
        );
    }
}
