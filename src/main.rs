// Veritas Kernel V0.2 - 主入口

mod engine;
mod types;
mod wal;
mod scope;
mod effect;
mod transaction;
mod store;
mod extension;

use types::*;
use engine::VeritasEngine;

fn main() {
    println!("╔════════════════════════════════════╗");
    println!("║   Veritas Kernel V0.2 - Phase 6    ║");
    println!("║   Extension 系统                   ║");
    println!("╚════════════════════════════════════╝\n");

    let engine = VeritasEngine::new();
    let account_a = deterministic_hash("Account::A::Balance");
    engine.init_state(account_a, 1000u64.to_le_bytes().to_vec());

    let mut tx = engine.begin();
    let val = engine.read(&mut tx, account_a).unwrap();
    let balance = u64::from_le_bytes(val[..8].try_into().unwrap());
    println!("[事务 {}] 读取余额: {}", tx.tx_id, balance);

    let _key = engine.effect(&mut tx, b"notification: balance checked".to_vec()).unwrap();
    engine.commit(&mut tx).unwrap();
    println!("[事务 {}] COMMIT + EFFECT 执行 ✓", tx.tx_id);

    println!("\n✓ Phase 6 Extension 系统已就绪");
}
