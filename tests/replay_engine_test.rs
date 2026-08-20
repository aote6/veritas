//! Replay Engine：能正确看到 birth 与 capability 事件。
//!
//! 验证内容：replay engine 观察到 ObjectBirth 与 Capability 相关事件。
//! 对应 VERIFICATION_MAP：replay_engine_test.rs
//! 若失败，意味着 replay 事件提取或引擎观察路径缺失。

use veritas_kernel::kernel::{Kernel, KernelCall, TrapResult};
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::types::ObjectType;

fn birth(kernel: &Kernel) -> u64 {
    let mut tx = kernel.test_begin();
    let id = match kernel
        .handle(
            &mut tx,
            KernelCall::ObjectBirth {
                object_type: ObjectType::StateObject,
            },
        )
        .unwrap()
    {
        TrapResult::ObjectId(id) => id,
        _ => panic!("expected ObjectId"),
    };
    kernel.handle(&mut tx, KernelCall::Commit);
    id
}

/// Replay engine 正确观察到 ObjectBirth 事件。
/// 失败意味着 birth 未进入 replay 可见事件流。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn replay_engine_sees_births() {
    let wal_path = format!("target/test_replay_birth_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let ids: Vec<u64>;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        ids = (0..3).map(|_| birth(&kernel)).collect();
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        for &id in &ids {
            assert_eq!(
                kernel.test_engine().get_object_state(id),
                Some(veritas_kernel::types::ObjectState::Alive)
            );
        }
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// Replay engine 正确观察到 Capability 相关事件。
/// 失败意味着 capability 事件未进入 replay 可见事件流。
/// @category: A
/// @layer: kernel
/// @testworld: FORBIDDEN
/// @req: DET-01
#[test]
fn replay_engine_sees_capability() {
    let wal_path = format!("target/test_replay_cap_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64;
    let holder: u64;
    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        owner = birth(&kernel);
        holder = birth(&kernel);
    }

    {
        let kernel = Kernel::with_wal_path(wal_path.clone());
        assert_eq!(
            kernel.test_engine().get_object_state(owner),
            Some(veritas_kernel::types::ObjectState::Alive)
        );
        assert_eq!(
            kernel.test_engine().get_object_state(holder),
            Some(veritas_kernel::types::ObjectState::Alive)
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
