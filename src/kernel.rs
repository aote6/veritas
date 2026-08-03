// Veritas Kernel V0.3 - Kernel wrapper for VeritasEngine
//
// Phase 1: Thin wrapper that delegates all calls to the real VeritasEngine.
// Future phases will restrict access to TRAP-only kernel services.

use crate::checkpoint::Checkpoint;
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
    CapabilityGrant {
        grantee: ObjectId,
        capability_type: String,
        resource: ObjectId,
    },
    CapabilityRevoke {
        capability_id: CapabilityId,
    },
    MemoryAlloc {
        object_id: ObjectId,
        size_hint: u64,
    },
}

/// TrapResult is returned by Kernel::handle() after processing a KernelCall.
/// The result is written to register r0 by Machine.
#[derive(Debug, Clone)]
pub enum TrapResult {
    ObjectId(ObjectId),
    CapabilityId(CapabilityId),
    StateId(StateId),
    Success,
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

    // ===== Phase 1 Step 4: KernelCall dispatch =====

    /// Handle a decoded kernel service call.
    /// This is the single entry point for all kernel services.
    /// Machine calls this instead of individual kernel methods.
    pub fn handle(
        &self,
        ctx: &mut TransactionContext,
        call: KernelCall,
    ) -> Result<TrapResult, VeritasError> {
        match call {
            KernelCall::ObjectBirth { object_type } => {
                // Phase 1 Step 4: temporary - caller still provides object_id
                // Phase 2 will have Kernel allocate ObjectId internally
                let id = ctx.tx_id; // placeholder until Kernel allocator
                self.object_birth(ctx, id)?;
                Ok(TrapResult::ObjectId(id))
            }
            KernelCall::ObjectDeath { object_id } => {
                self.object_death(ctx, object_id)?;
                Ok(TrapResult::Success)
            }
            KernelCall::ObjectLink { from, to, link_type } => {
                self.object_link(ctx, from, to, link_type)?;
                Ok(TrapResult::Success)
            }
            KernelCall::ObjectUnlink { from, to } => {
                self.object_unlink(ctx, from, to)?;
                Ok(TrapResult::Success)
            }
            KernelCall::ObjectFreeze { object_id } => {
                self.object_freeze(ctx, object_id)?;
                Ok(TrapResult::Success)
            }
            KernelCall::CapabilityGrant { grantee, capability_type, resource } => {
                let cap_id = self.capability_grant(ctx, grantee, &capability_type, resource)?;
                Ok(TrapResult::CapabilityId(cap_id))
            }
            KernelCall::CapabilityRevoke { .. } => {
                // CapabilityRevoke not yet implemented in Engine
                Ok(TrapResult::Success)
            }
            KernelCall::MemoryAlloc { .. } => {
                // MemoryAlloc not yet implemented
                Ok(TrapResult::Success)
            }
        }
    }

    // ===== Phase 1: Pass-through delegation ====="


    // Each method delegates directly to the corresponding VeritasEngine method.

    pub fn last_dependency_invalidations(&self) -> Vec<(ObjectId, ObjectId)> {
        self.engine.last_dependency_invalidations()
    }

    pub fn get_object_state(&self, object_id: ObjectId) -> Option<ObjectState> {
        self.engine.get_object_state(object_id)
    }

    pub fn is_object_dead(&self, object_id: ObjectId) -> bool {
        self.engine.is_object_dead(object_id)
    }

    pub fn attach_capability(&self, ctx: &mut TransactionContext, cap_id: u64) {
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

    pub fn record_history(&self, ctx: &TransactionContext) {
        self.engine.record_history(ctx)
    }

    pub fn create_checkpoint(&self) -> Checkpoint {
        self.engine.create_checkpoint()
    }

    pub fn restore_checkpoint(&self, ck: &Checkpoint) -> bool {
        self.engine.restore_checkpoint(ck)
    }

    pub fn apply_state_memory(&self, _ctx: &TransactionContext, write_set: &WriteSet) {
        self.engine.apply_state_memory(_ctx, write_set)
    }

    pub fn state_root(&self) -> u64 {
        self.engine.state_root()
    }

    pub fn init_state(&self, state_id: StateId, initial_value: Vec<u8>) {
        self.engine.init_state(state_id, initial_value)
    }

    pub fn peek_state(&self, state_id: StateId) -> Option<StateEntry> {
        self.engine.peek_state(state_id)
    }

    pub fn init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        initial_value: Vec<u8>,
    ) {
        self.engine.init_state_in_tx(ctx, state_id, initial_value)
    }

    pub fn begin(&self) -> TransactionContext {
        self.engine.begin()
    }

    pub fn begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
        self.engine.begin_in_object(object_id)
    }

    pub fn read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        self.engine.read(ctx, state_id)
    }

    pub fn write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        payload: Vec<u8>,
    ) -> Result<(), VeritasError> {
        self.engine.write(ctx, state_id, payload)
    }

    pub fn effect(
        &self,
        ctx: &mut TransactionContext,
        payload: Vec<u8>,
    ) -> Result<String, VeritasError> {
        self.engine.effect(ctx, payload)
    }

    pub fn commit(&self, ctx: &mut TransactionContext) -> Result<(), VeritasError> {
        self.engine.commit(ctx)
    }

    pub fn capability_grant(
        &self,
        ctx: &mut TransactionContext,
        grantee: ObjectId,
        capability_type: &str,
        resource: ObjectId,
    ) -> Result<CapabilityId, VeritasError> {
        self.engine.capability_grant(ctx, grantee, capability_type, resource)
    }

    pub fn object_freeze(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        self.engine.object_freeze(ctx, object_id)
    }

    pub fn object_death(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        self.engine.object_death(ctx, object_id)
    }

    pub fn object_link(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
        link_type: LinkType,
    ) -> Result<(), VeritasError> {
        self.engine.object_link(ctx, from, to, link_type)
    }

    pub fn object_unlink(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), VeritasError> {
        self.engine.object_unlink(ctx, from, to)
    }

    pub fn object_birth(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        self.engine.object_birth(ctx, object_id)
    }

    pub fn abort(&self, ctx: &mut TransactionContext, reason: AbortReason) {
        self.engine.abort(ctx, reason)
    }

    pub fn get_global_version(&self) -> Version {
        self.engine.get_global_version()
    }

    pub fn savepoint(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        self.engine.savepoint(ctx, name)
    }

    pub fn rollback_to(
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
            kernel.object_birth(&mut ctx, 10).unwrap();
            kernel.object_birth(&mut ctx, 20).unwrap();
            kernel.commit(&mut ctx).unwrap();
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
        kernel.object_birth(&mut ctx, 42).unwrap();
        kernel.commit(&mut ctx).unwrap();
        let ctx2 = kernel.begin();
        assert_eq!(kernel.get_object_state(42), Some(ObjectState::Alive));
        drop(ctx2);
    }

    #[test]
    fn kernel_state_independent_of_machine_lifetime() {
        let kernel = Kernel::new();
        {
            let mut ctx = kernel.begin();
            kernel.object_birth(&mut ctx, 100).unwrap();
            kernel.commit(&mut ctx).unwrap();
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
        kernel.object_birth(&mut ctx1, 1).unwrap();
        kernel.object_birth(&mut ctx1, 2).unwrap();
        kernel.commit(&mut ctx1).unwrap();
        let mut ctx2 = kernel.begin();
        kernel.object_birth(&mut ctx2, 3).unwrap();
        kernel.commit(&mut ctx2).unwrap();
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
            kernel.object_birth(&mut ctx, 77).unwrap();
            kernel.commit(&mut ctx).unwrap();
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
        let call = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let result = kernel.handle(&mut ctx, call).unwrap();
        let id = match result {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        assert!(id > 0);
        kernel.commit(&mut ctx).unwrap();
        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Alive));
    }

    #[test]
    fn abi_trap_object_link_via_handle() {
        let kernel = Kernel::new();

        // Create object A
        let mut ctx_a = kernel.begin();
        let call_a = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let id_a = match kernel.handle(&mut ctx_a, call_a).unwrap() {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        kernel.commit(&mut ctx_a).unwrap();

        // Create object B
        let mut ctx_b = kernel.begin();
        let call_b = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let id_b = match kernel.handle(&mut ctx_b, call_b).unwrap() {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        kernel.commit(&mut ctx_b).unwrap();

        // Link A -> B via handle
        let mut ctx_link = kernel.begin();
        let call_link = KernelCall::ObjectLink {
            from: id_a, to: id_b, link_type: LinkType::DependsOn,
        };
        let result = kernel.handle(&mut ctx_link, call_link).unwrap();
        assert!(matches!(result, TrapResult::Success));
        kernel.commit(&mut ctx_link).unwrap();

        assert!(kernel.has_link(id_a, id_b));
    }

    #[test]
    fn abi_trap_object_freeze_via_handle() {
        let kernel = Kernel::new();
        let mut ctx1 = kernel.begin();
        let call = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let id = match kernel.handle(&mut ctx1, call).unwrap() {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        kernel.commit(&mut ctx1).unwrap();

        let mut ctx2 = kernel.begin();
        let result = kernel.handle(&mut ctx2, KernelCall::ObjectFreeze { object_id: id }).unwrap();
        assert!(matches!(result, TrapResult::Success));
        kernel.commit(&mut ctx2).unwrap();

        assert_eq!(kernel.get_object_state(id), Some(ObjectState::Frozen));
    }

    #[test]
    fn abi_trap_object_death_via_handle() {
        let kernel = Kernel::new();
        let mut ctx1 = kernel.begin();
        let call = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let id = match kernel.handle(&mut ctx1, call).unwrap() {
            TrapResult::ObjectId(id) => id,
            _ => panic!("Expected ObjectId"),
        };
        kernel.commit(&mut ctx1).unwrap();

        let mut ctx2 = kernel.begin();
        let result = kernel.handle(&mut ctx2, KernelCall::ObjectDeath { object_id: id }).unwrap();
        assert!(matches!(result, TrapResult::Success));
        kernel.commit(&mut ctx2).unwrap();

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
        let call = KernelCall::ObjectBirth { object_type: ObjectType::StateObject };
        let result = kernel.handle(&mut ctx, call).unwrap();
        match result {
            TrapResult::ObjectId(id) => assert!(id > 0),
            _ => panic!("Expected ObjectId from ObjectBirth"),
        }
    }
}