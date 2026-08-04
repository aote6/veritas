# Architecture Inventory

日期: 2026-08-04

## 保留: 25个

types.rs engine.rs kernel.rs machine.rs capability.rs store.rs
scope_registry.rs wal.rs lock.rs tx_manager.rs controller.rs
instruction.rs instruction_codec.rs execution.rs executor.rs
program.rs assembler.rs module.rs effect.rs event.rs guard.rs
verifier.rs receipt.rs trace.rs view.rs runtime.rs extension.rs
memory.rs scope.rs

scope.rs 判定: API Extension (ScopeExt trait), 不持有状态, 保留

## 待迁移后删除: 5个

state_memory.rs - 第二套状态, 已被 StateStore+Checkpoint 取代
history.rs - 旧 Replay 系统 ExecutionHistory/ReplayRecord
replay.rs - 旧 ReplayEngine, 基于 state_memory
replay_verify.rs - 旧 Replay 验证
checkpoint.rs - 旧 Checkpoint, 已被 WorldSnapshot 取代

## 直接删除: 1个

engine.rs.patch - 临时补丁文件

## 迁移顺序

1. state_root() 迁移到 StateStore (唯一根哈希)
2. record_history() 退役
3. 引用清零
4. 删除5+1文件
5. 测试清理, cargo test 全绿
