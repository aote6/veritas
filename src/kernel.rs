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

    // ===== Phase 1: Pass-through delegation =====
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

    #[test]
    fn test_kernel_object_birth() {
        let kernel = Kernel::new();
        let mut ctx = kernel.begin();
        kernel.object_birth(&mut ctx, 42).unwrap();
        kernel.commit(&mut ctx).unwrap();
        assert_eq!(kernel.get_object_state(42), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_survives_multiple_machine_runs() {
        // Phase 1 Step 3: Kernel survives beyond any single Machine.
        // This is now fully implemented with Arc<Kernel>.
        use std::sync::Arc;

        let kernel = Arc::new(Kernel::new());

        // Machine 1 creates objects
        {
            let _machine1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.object_birth(&mut ctx, 10).unwrap();
            kernel.object_birth(&mut ctx, 20).unwrap();
            kernel.commit(&mut ctx).unwrap();
            // Machine 1 dropped here
        }

        // Machine 2 sees all objects (same kernel world)
        {
            let _machine2 = crate::machine::Machine::new(Arc::clone(&kernel));
            assert_eq!(kernel.get_object_state(10), Some(ObjectState::Alive));
            assert_eq!(kernel.get_object_state(20), Some(ObjectState::Alive));
            // Machine 2 dropped here
        }

        // Kernel world persists independently
        assert_eq!(kernel.get_object_state(10), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(20), Some(ObjectState::Alive));
    }

    #[test]
    fn kernel_persists_object_across_machines() {
        // Create one kernel, use it with two sequential machines
        let kernel = Kernel::new();

        // Machine 1: create an object
        let mut ctx = kernel.begin();
        kernel.object_birth(&mut ctx, 42).unwrap();
        kernel.commit(&mut ctx).unwrap();

        // Machine 2: read the object (same kernel)
        let ctx2 = kernel.begin();
        // Object 42 should exist because kernel is the same world
        assert_eq!(kernel.get_object_state(42), Some(ObjectState::Alive));
        drop(ctx2);
    }

    #[test]
    fn kernel_state_independent_of_machine_lifetime() {
        let kernel = Kernel::new();

        // Create object through a temporary machine
        {
            let mut ctx = kernel.begin();
            kernel.object_birth(&mut ctx, 100).unwrap();
            kernel.commit(&mut ctx).unwrap();
        }
        // 'machine' is gone, but kernel and its objects persist

        // Verify object still exists
        assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));

        // Create another machine, verify it sees the same world
        let ctx = kernel.begin();
        assert_eq!(kernel.get_object_state(100), Some(ObjectState::Alive));
        drop(ctx);
    }

    #[test]
    fn kernel_object_id_monotonic_across_machines() {
        let kernel = Kernel::new();

        // Machine 1 creates objects
        let mut ctx1 = kernel.begin();
        kernel.object_birth(&mut ctx1, 1).unwrap();
        kernel.object_birth(&mut ctx1, 2).unwrap();
        kernel.commit(&mut ctx1).unwrap();

        // Machine 2 creates more objects (same kernel world)
        let mut ctx2 = kernel.begin();
        kernel.object_birth(&mut ctx2, 3).unwrap();
        kernel.commit(&mut ctx2).unwrap();

        // All three objects should exist in the same world
        assert_eq!(kernel.get_object_state(1), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(2), Some(ObjectState::Alive));
        assert_eq!(kernel.get_object_state(3), Some(ObjectState::Alive));
    }


    #[test]
    fn kernel_shared_by_multiple_machines() {
        // Phase 1 Step 3 verification: multiple machines share one Kernel world.
        use std::sync::Arc;

        let kernel = Arc::new(Kernel::new());

        // Machine 1 creates an object
        {
            let mut machine1 = crate::machine::Machine::new(Arc::clone(&kernel));
            let mut ctx = kernel.begin();
            kernel.object_birth(&mut ctx, 77).unwrap();
            kernel.commit(&mut ctx).unwrap();
            drop(machine1);
        }

        // Machine 2 sees the object (same kernel world)
        {
            let machine2 = crate::machine::Machine::new(Arc::clone(&kernel));
            assert_eq!(kernel.get_object_state(77), Some(ObjectState::Alive));
            drop(machine2);
        }

        // Object 77 persists beyond both machines
        assert_eq!(kernel.get_object_state(77), Some(ObjectState::Alive));
    }

}
