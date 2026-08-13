//! Integration-test support for Kernel mutation helpers.
//!
//! Production Runtime must not use this module.
//! External mutation path: WorldService → Kernel::handle / pub(crate) helpers.
//! Machine (same crate) uses Kernel pub(crate) methods directly.

use crate::engine::VeritasEngine;
use crate::kernel::Kernel;
use crate::types::*;

/// Extension trait exposing test-only mutation helpers on Kernel.
/// Production code paths use `Kernel::handle` or `WorldService`.
pub trait KernelTestExt {
    fn test_begin(&self) -> TransactionContext;
    fn test_begin_in_object(&self, object_id: ObjectId) -> TransactionContext;
    fn test_read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError>;
    fn test_write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        payload: Vec<u8>,
    ) -> Result<(), VeritasError>;
    fn test_commit(
        &self,
        ctx: &mut TransactionContext,
    ) -> Result<TransactionReceipt, VeritasError>;
    fn test_effect(
        &self,
        ctx: &mut TransactionContext,
        payload: Vec<u8>,
    ) -> Result<String, VeritasError>;
    fn test_savepoint(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError>;
    fn test_rollback_to(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError>;
    fn test_attach_capability(&self, ctx: &mut TransactionContext, cap_id: u64);
    fn test_engine(&self) -> &VeritasEngine;
    fn test_capability_records(&self) -> Vec<crate::types::CapabilitySemanticRecord>;
    fn test_init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    );
    fn test_authorize_intent(
        &self,
        ctx: &TransactionContext,
        intent: &AccessIntent,
    ) -> Result<(), VeritasError>;
}

impl KernelTestExt for Kernel {
    fn test_begin(&self) -> TransactionContext {
        self.begin()
    }
    fn test_begin_in_object(&self, object_id: ObjectId) -> TransactionContext {
        self.begin_in_object(object_id)
    }
    fn test_read(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
    ) -> Result<Vec<u8>, VeritasError> {
        self.read(ctx, state_id)
    }
    fn test_write(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        payload: Vec<u8>,
    ) -> Result<(), VeritasError> {
        self.write(ctx, state_id, payload)
    }
    fn test_commit(
        &self,
        ctx: &mut TransactionContext,
    ) -> Result<TransactionReceipt, VeritasError> {
        self.commit(ctx)
    }
    fn test_effect(
        &self,
        ctx: &mut TransactionContext,
        payload: Vec<u8>,
    ) -> Result<String, VeritasError> {
        self.effect(ctx, payload)
    }
    fn test_savepoint(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        self.savepoint(ctx, name)
    }
    fn test_rollback_to(
        &self,
        ctx: &mut TransactionContext,
        name: &str,
    ) -> Result<(), VeritasError> {
        self.rollback_to(ctx, name)
    }
    fn test_attach_capability(&self, ctx: &mut TransactionContext, cap_id: u64) {
        self.attach_capability(ctx, cap_id)
    }
    fn test_engine(&self) -> &VeritasEngine {
        self.engine()
    }
    fn test_capability_records(&self) -> Vec<crate::types::CapabilitySemanticRecord> {
        self.engine().snapshot_capabilities_for_test()
    }
    fn test_init_state_in_tx(
        &self,
        ctx: &mut TransactionContext,
        state_id: StateId,
        value: Vec<u8>,
    ) {
        self.init_state_in_tx(ctx, state_id, value)
    }
    fn test_authorize_intent(
        &self,
        ctx: &TransactionContext,
        intent: &AccessIntent,
    ) -> Result<(), VeritasError> {
        self.engine().authorize_intent(ctx, intent)
    }
}

/// Recover an engine from WAL for recovery equivalence tests.
/// Production recovery goes through Kernel::with_wal_path.
pub fn recover_engine(wal_path: &str) -> VeritasEngine {
    VeritasEngine::with_wal_path(wal_path.to_string())
}

/// Empty engine for pure component roundtrip tests (no WAL).
pub fn empty_engine() -> VeritasEngine {
    VeritasEngine::empty()
}
