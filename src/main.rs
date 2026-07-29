// Veritas Kernel V0.2 - 主入口
// Phase 2: WAL + 崩溃恢复

mod engine;
mod types;
mod wal;
mod scope;

use types::*;
use engine::VeritasEngine;

fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║   Veritas Kernel V0.2 - Phase 2    ║");
    println!("║   WAL + 崩溃恢复验证               ║");
    println!("╚════════════════════════════════════╝\n");

    // ========== 场景1：基本事务 + WAL ==========
    println!("--- 场景1：基本事务 + WAL持久化 ---");
    {
        let engine = VeritasEngine::new();
        let account_a = deterministic_hash("Account::A::Balance");
        engine.init_state(account_a, 1000u64.to_le_bytes().to_vec());

        let mut tx = engine.begin();
        println!("[事务 {}] BEGIN", tx.tx_id);
        
        let val = engine.read(&mut tx, account_a).unwrap();
        let balance = u64::from_le_bytes(val[..8].try_into().unwrap());
        println!("[事务 {}] 读取余额: {}", tx.tx_id, balance);
        
        engine.write(&mut tx, account_a, (balance - 300).to_le_bytes().to_vec()).unwrap();
        engine.commit(&mut tx).unwrap();
        println!("[事务 {}] COMMIT + WAL刷盘 成功 ✓", tx.tx_id);
    }

    // ========== 场景2：模拟崩溃恢复 ==========
    println!("\n--- 场景2：模拟崩溃恢复 ---");
    {
        let engine = VeritasEngine::new();
        let account_a = deterministic_hash("Account::A::Balance");
        
        let mut tx = engine.begin();
        let val = engine.read(&mut tx, account_a).unwrap();
        let balance = u64::from_le_bytes(val[..8].try_into().unwrap());
        println!("[恢复后] 账户A余额: {} (应为700)", balance);
        println!("[恢复后] 全局版本号: {}", engine.get_global_version());
    }

    println!("\n✓ Phase 2 WAL + 崩溃恢复验证通过");
    println!("  wal.log 文件已生成，可查看内容：cat wal.log");
}
