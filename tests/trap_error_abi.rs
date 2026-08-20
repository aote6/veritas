//! TRAP ABI Error Contract Tests
//!
//! 验证 VeritasError -> TrapResult::Error(code) -> TrapReason 的完整映射链。
//! 对应 docs/TRAP_ABI_ERROR_CONTRACT_FREEZE.md
//! 红线：WriteConflict 不能映射成 IllegalInstruction，
//!       StateNotFound 不能映射成 InvalidEncoding，
//!       未知 code 不能映射成 InvalidEncoding。

use veritas_kernel::kernel::{TrapResult, TRAP_ERR_ACCESS_DENIED, TRAP_ERR_ENGINE, TRAP_ERR_MEMORY_FAULT, TRAP_ERR_PERMISSION_DENIED, TRAP_ERR_STATE_NOT_FOUND, TRAP_ERR_WRITE_CONFLICT};
use veritas_kernel::types::{AbortReason, TrapReason, VeritasError};

fn map_code_for_test(code: u8, pc: usize) -> TrapReason {
    match code {
        TRAP_ERR_ACCESS_DENIED => TrapReason::AccessDenied { pc },
        TRAP_ERR_ENGINE => TrapReason::EngineError { pc },
        TRAP_ERR_MEMORY_FAULT => TrapReason::UnknownKernelError { code, pc },
        TRAP_ERR_WRITE_CONFLICT => TrapReason::WriteConflict { pc },
        TRAP_ERR_PERMISSION_DENIED => TrapReason::AccessDenied { pc },
        TRAP_ERR_STATE_NOT_FOUND => TrapReason::StateNotFound { pc },
        _ => TrapReason::UnknownKernelError { code, pc },
    }
}

#[test]
fn permission_denied_maps_to_access_denied() {
    let result = TrapResult::from_error(VeritasError::PermissionDenied);
    match result {
        TrapResult::Error(code) => {
            assert_eq!(code, TRAP_ERR_PERMISSION_DENIED);
            match map_code_for_test(code, 42) {
                TrapReason::AccessDenied { pc } => assert_eq!(pc, 42),
                other => panic!("expected AccessDenied, got {:?}", other),
            }
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn engine_error_maps_to_engine_error() {
    let result = TrapResult::from_error(VeritasError::EngineError("test".into()));
    match result {
        TrapResult::Error(code) => {
            assert_eq!(code, TRAP_ERR_ENGINE);
            match map_code_for_test(code, 7) {
                TrapReason::EngineError { pc } => assert_eq!(pc, 7),
                other => panic!("expected EngineError, got {:?}", other),
            }
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn write_conflict_does_not_become_illegal_instruction() {
    let result = TrapResult::from_error(VeritasError::Abort(AbortReason::WriteConflict));
    match result {
        TrapResult::Error(code) => {
            assert_eq!(code, TRAP_ERR_WRITE_CONFLICT);
            let reason = map_code_for_test(code, 10);
            assert!(
                !matches!(reason, TrapReason::IllegalInstruction { .. }),
                "WriteConflict must NOT map to IllegalInstruction"
            );
            assert!(
                matches!(reason, TrapReason::WriteConflict { pc } if pc == 10),
                "WriteConflict must map to WriteConflict, got {:?}",
                reason
            );
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn state_not_found_does_not_become_invalid_encoding() {
    let result = TrapResult::from_error(VeritasError::Abort(AbortReason::StateNotFound));
    match result {
        TrapResult::Error(code) => {
            assert_eq!(code, TRAP_ERR_STATE_NOT_FOUND);
            let reason = map_code_for_test(code, 11);
            assert!(
                !matches!(reason, TrapReason::InvalidEncoding { .. }),
                "StateNotFound must NOT map to InvalidEncoding"
            );
            assert!(
                matches!(reason, TrapReason::StateNotFound { pc } if pc == 11),
                "StateNotFound must map to StateNotFound, got {:?}",
                reason
            );
        }
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
                assert_eq!(code, TRAP_ERR_WRITE_CONFLICT, "all transaction abort reasons must map to WRITE_CONFLICT class");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }
}

#[test]
fn unknown_code_maps_to_unknown_kernel_error() {
    let unknown_code = 99u8;
    let reason = map_code_for_test(unknown_code, 23);
    match reason {
        TrapReason::UnknownKernelError { code, pc } => {
            assert_eq!(code, unknown_code);
            assert_eq!(pc, 23);
        }
        other => panic!("expected UnknownKernelError, got {:?}", other),
    }
}

#[test]
fn reserved_memory_fault_code_does_not_fabricate_addr_size() {
    let reason = map_code_for_test(TRAP_ERR_MEMORY_FAULT, 5);
    assert!(
        !matches!(reason, TrapReason::MemoryFault { .. }),
        "reserved MEMORY_FAULT code must NOT fabricate MemoryFault with addr=0 size=0"
    );
    match reason {
        TrapReason::UnknownKernelError { code, pc } => {
            assert_eq!(code, TRAP_ERR_MEMORY_FAULT);
            assert_eq!(pc, 5);
        }
        other => panic!("expected UnknownKernelError for reserved code, got {:?}", other),
    }
}
