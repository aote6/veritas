// Veritas Kernel V0.1 - 主入口
// Phase 1.1: 事务内核验证（含读自己的写、盲写保护）

mod engine;
mod types;

use types::*;
use engine::VeritasEngine;

fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║   Veritas Kernel V0.1 - Phase 1.1  ║");
    println!("║   事务内核验证（含盲写保护）        ║");
    println!("╚════════════════════════════════════╝\n");

    let engine = VeritasEngine::new();

    let account_a = deterministic_hash("Account::A::Balance");
    let account_b = deterministic_hash("Account::B::Balance");

    engine.init_state(account_a, 1000u64.to_le_bytes().to_vec());
    engine.init_state(account_b, 500u64.to_le_bytes().to_vec());

    println!("[初始化] 账户A余额: 1000, 账户B余额: 500\n");

    // ========== 转账演示 ==========
    println!("--- 转账演示 ---");
    let mut tx = engine.begin();
    println!("[事务 {}] BEGIN (快照版本: {})", tx.tx_id, tx.snapshot_version);

    let val_a = engine.read(&mut tx, account_a).unwrap();
    let balance_a = u64::from_le_bytes(val_a[..8].try_into().unwrap());
    let val_b = engine.read(&mut tx, account_b).unwrap();
    let balance_b = u64::from_le_bytes(val_b[..8].try_into().unwrap());
    println!("[事务 {}] 读取 A={}, B={}", tx.tx_id, balance_a, balance_b);

    engine.write(&mut tx, account_a, (balance_a - 300).to_le_bytes().to_vec()).unwrap();
    engine.write(&mut tx, account_b, (balance_b + 300).to_le_bytes().to_vec()).unwrap();
    println!("[事务 {}] 写入 A={}, B={}", tx.tx_id, balance_a - 300, balance_b + 300);

    match engine.commit(&mut tx) {
        Ok(()) => println!("[事务 {}] COMMIT 成功 ✓", tx.tx_id),
        Err(e) => println!("[事务 {}] COMMIT 失败: {:?}", tx.tx_id, e),
    }

    // ========== 盲写冲突演示 ==========
    println!("\n--- 盲写冲突演示 ---");
    let state_x = deterministic_hash("Counter::X::Value");
    engine.init_state(state_x, 0u64.to_le_bytes().to_vec());

    // 事务T1：盲写X=100（不先读）
    let mut t1 = engine.begin();
    engine.write(&mut t1, state_x, 100u64.to_le_bytes().to_vec()).unwrap();

    // 事务T2：盲写X=200（不先读），先提交
    let mut t2 = engine.begin();
    engine.write(&mut t2, state_x, 200u64.to_le_bytes().to_vec()).unwrap();
    engine.commit(&mut t2).unwrap();
    println!("[事务T2] 盲写X=200, COMMIT 成功");

    // T1提交——盲写保护检测到冲突
    match engine.commit(&mut t1) {
        Ok(()) => println!("[事务T1] 盲写X=100, COMMIT 成功（不应该发生）"),
        Err(e) => println!("[事务T1] 盲写X=100, COMMIT 失败: {:?} ✓ 盲写保护生效", e),
    }

    // 验证最终值
    let mut tx_final = engine.begin();
    let final_x = u64::from_le_bytes(
        engine.read(&mut tx_final, state_x).unwrap()[..8].try_into().unwrap()
    );
    println!("[最终值] X={} (应为200，未被T1的100覆盖)", final_x);

    // ========== 最终状态 ==========
    let mut tx2 = engine.begin();
    let final_a = u64::from_le_bytes(
        engine.read(&mut tx2, account_a).unwrap()[..8].try_into().unwrap()
    );
    let final_b = u64::from_le_bytes(
        engine.read(&mut tx2, account_b).unwrap()[..8].try_into().unwrap()
    );
    println!("\n[最终状态] 账户A: {}, 账户B: {}", final_a, final_b);
    println!("[全局版本号] {}", engine.get_global_version());
    println!("\n✓ Phase 1.1 事务内核验证通过");
}
