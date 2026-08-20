//! TRAP ABI Error Contract Tests — VeritasError → TrapResult::Error(code)
//!
//! 本文件只验证 VeritasError 到 TrapResult::Error(code) 的映射。
//! TrapResult::Error(code) → TrapReason 的映射在 src/machine.rs 的
//! unit tests（trap_mapping_tests）中验证，直接测试生产代码 map_trap_code()。
//!
//! 完整映射链：
//!   VeritasError → TrapResult::Error(code)  [本文件]
//!   TrapResult::Error(code) → TrapReason    [src/machine.rs unit tests]
//!
//! 对应 docs/TRAP_ABI_ERROR_CONTRACT_FREEZE.md

use veritas_kernel::kernel::{
    TrapResult, TRAP_ERR_ENGINE, TRAP_ERR_PERMISSION_DENIED, TRAP_ERR_STATE_NOT_FOUND,
    TRAP_ERR_WRITE_CONFLICT,
};
use veritas_kernel::types::{AbortReason, VeritasError};

#[test]
fn permission_denied_maps_to_code_5() {
    let result = TrapResult::from_error(VeritasError::PermissionDenied);
    match result {
        TrapResult::Error(code) => assert_eq!(code, TRAP_ERR_PERMISSION_DENIED),
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn engine_error_maps_to_code_2() {
    let result = TrapResult::from_error(VeritasError::EngineError("test".into()));
    match result {
        TrapResult::Error(code) => assert_eq!(code, TRAP_ERR_ENGINE),
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn write_conflict_maps_to_code_4() {
    let result = TrapResult::from_error(VeritasError::Abort(AbortReason::WriteConflict));
    match result {
        TrapResult::Error(code) => assert_eq!(code, TRAP_ERR_WRITE_CONFLICT),
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn state_not_found_maps_to_code_6() {
    let result = TrapResult::from_error(VeritasError::Abort(AbortReason::StateNotFound));
    match result {
        TrapResult::Error(code) => assert_eq!(code, TRAP_ERR_STATE_NOT_FOUND),
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn all_abort_reasons_map_to_write_conflict_class() {
    let reasons = vec![
        AbortReason::WriteConflict,
        AbortReason::ReadFutureVersion,
        AbortReason::AlreadyAborted,
        AbortReason::PhantomConflict,
    ];
    for reason in reasons {
        let result = TrapResult::from_error(VeritasError::Abort(reason));
        match result {
            TrapResult::Error(code) => {
                assert_eq!(
                    code, TRAP_ERR_WRITE_CONFLICT,
                    "all transaction abort reasons must map to WRITE_CONFLICT class"
                );
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }
}

#[test]
fn determinism_violation_maps_to_write_conflict_class() {
    let result = TrapResult::from_error(VeritasError::DeterminismViolation);
    match result {
        TrapResult::Error(code) => assert_eq!(code, TRAP_ERR_WRITE_CONFLICT),
        other => panic!("expected Error, got {:?}", other),
    }
}
