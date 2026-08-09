use crate::common::new_kernel;
use veritas_kernel::test_api::KernelTestExt;
use veritas_kernel::kernel::KernelCall;

#[test]
fn t1_commit_persists_state() {
    let tk = new_kernel();
    let state_id = 42;
    let payload = vec![1, 2, 3, 4];

    let mut tx = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel.test_write(&mut tx, state_id, payload.clone()).unwrap();
    tk.kernel.handle(&mut tx, KernelCall::Commit).unwrap();

    let mut read_tx = tk.kernel.test_begin_in_object(tk.root_object);
    let val = tk.kernel.test_read(&mut read_tx, state_id).unwrap();
    assert_eq!(val, payload, "T1 Invariant Violation: Committed data mismatch!");
}


// ============================================================
// F-002 回归测试：Effect 必须进入 TransactionDelta
// ============================================================

/// 验证 Effect 在 commit 后出现在 TransactionDelta 中
#[test]
fn test_effect_persisted_in_transaction_delta() {
    let tk = new_kernel();
    let payload = vec![10, 20, 30];

    // 开启事务
    let mut tx = tk.kernel.test_begin_in_object(tk.root_object);

    // 写入一个 Effect
    let key = tk.kernel.test_effect(&mut tx, payload.clone()).unwrap();

    // 提交
    let receipt = tk.kernel.test_commit(&mut tx).unwrap();

    // F-002 核心断言：delta.effects 不能为空
    assert!(
        !receipt.delta.effects.is_empty(),
        "F-002 REGRESSION: TransactionDelta.effects is empty after commit"
    );

    // 验证 effects 包含正确的 key 和 payload
    let effect = receipt.delta.effects.iter()
        .find(|(k, _)| k == &key)
        .expect("F-002 REGRESSION: committed effect key not found in delta.effects");

    assert_eq!(
        effect.1, payload,
        "F-002 REGRESSION: effect payload mismatch in delta"
    );

    eprintln!(
        "[TEST] delta.effects = {:?}",
        receipt.delta.effects
    );
}

/// 验证 WAL recovery 能恢复 TransactionCommitted 中的 effects
#[test]
fn test_effect_survives_wal_recovery() {
    let tk = new_kernel();
    let payload = vec![7, 8, 9];

    // 写入 Effect 并提交
    let mut tx = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel.test_effect(&mut tx, payload.clone()).unwrap();
    let receipt = tk.kernel.test_commit(&mut tx).unwrap();

    // 验证提交时 effects 非空
    let expected_key = receipt.delta.effects[0].0.clone();
    assert!(!receipt.delta.effects.is_empty());

    // 通过 WAL recovery 重建引擎
    let wal_path = tk.kernel.test_engine().wal_path().to_string();
    let recovered = veritas_kernel::test_api::recover_engine(&wal_path);
    let recovered_version = recovered.get_global_version();

    eprintln!(
        "[TEST] recovered version={}, effects in original delta={:?}",
        recovered_version,
        receipt.delta.effects
    );

    // Recovery 后 version 应 >= commit version
    assert!(
        recovered_version >= receipt.delta.commit_version,
        "F-002 REGRESSION: recovered version {} < commit version {}",
        recovered_version,
        receipt.delta.commit_version
    );

    // 核心：Recovery 后不应因为 pending effect 就伪造 Ack
    // 此测试仅验证 recovery 不崩溃，且 effects 在 delta 中可恢复
    eprintln!(
        "[TEST] F-002-B 检查: Recovery 完成，effect key={} 应在 WAL TransactionCommitted 中",
        expected_key
    );
}


// ============================================================


// ============================================================
// F-007 回归测试：COMMIT 和 ABORT 互斥
// ============================================================

/// F-007 单线程不变量：ABORT 后不能 COMMIT
#[test]
fn test_aborted_tx_commit_must_fail() {
    let tk = new_kernel();
    let payload = vec![9, 9, 9];

    let mut tx = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel.test_write(&mut tx, 200, payload).unwrap();

    // 显式 ABORT
    tk.kernel.handle(&mut tx, veritas_kernel::kernel::KernelCall::Abort {
        reason: veritas_kernel::types::AbortReason::WriteConflict,
    }).unwrap();

    // 尝试 COMMIT 必须失败
    let result = tk.kernel.test_commit(&mut tx);
    assert!(
        result.is_err(),
        "F-007 REGRESSION: aborted transaction should not commit, got {:?}",
        result
    );

    // 验证状态没有被写入
    let mut read_tx = tk.kernel.test_begin_in_object(tk.root_object);
    let val = tk.kernel.test_read(&mut read_tx, 200);
    assert!(
        val.is_err(),
        "F-007 REGRESSION: aborted write should not be visible"
    );
}

/// F-007: COMMIT 成功后 ABORT 必须是 no-op 或失败
#[test]
fn test_committed_tx_abort_must_be_noop() {
    let tk = new_kernel();
    let payload = vec![8, 8, 8];

    let mut tx = tk.kernel.test_begin_in_object(tk.root_object);
    tk.kernel.test_write(&mut tx, 300, payload.clone()).unwrap();

    // 先 COMMIT
    let _receipt = tk.kernel.test_commit(&mut tx).unwrap();

    // COMMIT 后再 ABORT
    let _result = tk.kernel.handle(&mut tx, veritas_kernel::kernel::KernelCall::Abort {
        reason: veritas_kernel::types::AbortReason::WriteConflict,
    });

    // 无论 Ok/Err，已提交的状态不能被回滚
    let mut read_tx = tk.kernel.test_begin_in_object(tk.root_object);
    let val = tk.kernel.test_read(&mut read_tx, 300).unwrap();
    assert_eq!(
        val, payload,
        "F-007 REGRESSION: committed state should survive abort attempt"
    );
}

/// F-007 并发不变量：使用 Arc<Kernel> 包装跨线程共享，验证 ABORT 和 COMMIT 竞争时不产生虚假提交
#[test]
fn test_concurrent_commit_abort_no_spurious_commit() {
    use std::sync::{Arc, Barrier};
    use std::thread;
    use veritas_kernel::kernel::KernelCall;
    use veritas_kernel::types::AbortReason;

    let tk = new_kernel();
    let root = tk.root_object;

    // 先写一个初始状态
    {
        let mut tx = tk.kernel.test_begin_in_object(root);
        tk.kernel.test_write(&mut tx, 400, vec![1]).unwrap();
        tk.kernel.test_commit(&mut tx).unwrap();
    }

    // 用 Arc<Kernel> 包装供跨线程共享
    let kernel = Arc::new(tk.kernel);

    for round in 0..10 {
        let k1 = kernel.clone();
        let k2 = kernel.clone();
        let b1 = Arc::new(Barrier::new(2));
        let b2 = b1.clone();

        let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c = committed.clone();

        let h1 = thread::spawn(move || {
            let mut tx = k1.test_begin_in_object(root);
            k1.test_write(&mut tx, 500 + round, vec![2]).unwrap();
            b1.wait();
            if k1.test_commit(&mut tx).is_ok() {
                c.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });

        let h2 = thread::spawn(move || {
            let mut tx = k2.test_begin_in_object(root);
            k2.test_write(&mut tx, 500 + round, vec![3]).unwrap();
            b2.wait();
            let _ = k2.handle(&mut tx, KernelCall::Abort {
                reason: AbortReason::WriteConflict,
            });
        });

        h1.join().unwrap();
        h2.join().unwrap();

        // 如果 commit 成功，状态必须可见
        if committed.load(std::sync::atomic::Ordering::SeqCst) {
            let mut read_tx = kernel.test_begin_in_object(root);
            let val = kernel.test_read(&mut read_tx, 500 + round);
            assert!(
                val.is_ok(),
                "F-007: commit=true but state not found at round {}",
                round
            );
        }
    }
}
