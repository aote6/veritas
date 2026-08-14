# Veritas 验证地图（自动生成）

生成日期: 2026-08-14
生成方式: python3 gen_verification_map.py
数据来源: tests/*.rs 的 //! 文档注释和 #[test] 函数名

此文件由脚本自动生成。不要手动编辑。
如果发现缺了内容，应该修改对应测试文件的 //! 注释，然后重新生成。

---

### call_access_intent.rs

**验证内容**:

- P3: CALL → AccessIntent unification tests.

**测试函数** (8 个):

- call_without_capability_fails
- call_with_capability_succeeds
- call_after_delegate_succeeds
- call_after_revoke_fails
- call_permission_survives_checkpoint
- call_permission_survives_wal_replay
- call_intent_collected_in_verify_path
- call_self_is_exempt

---

### capability_delegate_p4_recovery.rs

**验证内容**:

- P4: CapabilityDelegate WAL Closure — topology recovery equivalence.

**测试函数** (7 个):

- t1_delegate_survives_checkpoint
- t2_multilevel_delegate_tree
- t3_cascade_revoke
- t4_non_cascade_revoke
- t5_wal_replay_equals_checkpoint_and_live
- t6_rollback_drops_pending_delegate
- t7_old_wal_without_capdelegate_compatible

---

### capability_grant_cross_object.rs

**测试函数** (1 个):

- grantor_is_real_authorizer_not_self_grant

---

### capability_grant_p1_jsonlines.rs

**测试函数** (1 个):

- tx_capability_grant_jsonl_end_to_end

---

### capability_grant_p1_worldapi.rs

**测试函数** (1 个):

- tx_capability_grant_external_interface_end_to_end

---

### capability_p4x_recovery.rs

**测试函数** (3 个):

- capability_grant_visible_after_commit
- capability_survives_recovery
- capability_grant_no_leak_on_abort

---

### capability_revoke.rs

**验证内容**:

- P2: CapabilityRevoke Kernel → Engine → Graph → WAL/Checkpoint closure.

**测试函数** (5 个):

- kernel_capability_revoke_cascade_downstream
- kernel_capability_revoke_non_cascade_preserves_downstream
- kernel_capability_revoke_survives_checkpoint
- kernel_capability_revoke_wal_replay
- kernel_capability_revoke_not_holder_errors

---

### checkpoint_continuity.rs

**测试函数** (4 个):

- checkpoint_restore_world_continuity
- capability_identity_survives_checkpoint_restore
- object_death_no_ghost_state_after_checkpoint
- checkpoint_preserves_state_entry_versions

---

### checkpoint_roundtrip.rs

**测试函数** (5 个):

- checkpoint_full_roundtrip_all_five_components
- checkpoint_restore_then_continue_execution
- checkpoint_restore_idempotent
- checkpoint_root_hash_consistent
- checkpoint_counter_roundtrip

---

### commitment_domain.rs

**测试函数** (2 个):

- diagnose_live_vs_recovery_components
- self_access_does_not_grow_capability_graph

---

### forge_e2e_jsonlines.rs

**测试函数** (1 个):

- forge_e2e_create_write_read_commit_observe

---

### freeze_unlink_p5x_recovery.rs

**测试函数** (5 个):

- freeze_then_death_survives_recovery
- link_then_unlink_survives_recovery
- freeze_and_unlink_survives_recovery
- unlink_then_death_target_survives
- death_cascade_survives_recovery

---

### kernel_world_runtime.rs

**测试函数** (1 个):

- module_a_object_visible_to_module_b_through_runtime_execute

---

### machine_object_link_security.rs

**验证内容**:

- P0 安全回归测试：防止 OBJECT_LINK 身份伪造漏洞重现。
- 历史 bug：machine.rs 的 ObjectLink 分支曾经在调用 kernel 之前
- 执行 `self.ctx.enter_object(from)`，导致 commit 时的
- authorize_intent(AccessIntent::Link(from, to)) 检查里
- `target == ctx.current_object` 恒真，从而让调用者无需持有
- 任何 capability 就能伪造任意两个对象之间的 Link。
- 本文件锁定两件事：
- 1. 恶意路径：调用者对 from/to 均无 capability 时，Link 必须被拒绝。
- 2. 合法路径：调用者持有正确 capability 时，Link 仍然必须成功
- （证明 P0 修复没有误伤正常授权流程）。

**测试函数** (2 个):

- object_link_without_capability_on_target_is_rejected
- object_link_with_proper_capability_succeeds

---

### multi_object_transaction_matrix.rs

**验证内容**:

- Multi-object transaction permutation matrix (ROADMAP §3).
- Only tests. No production-code changes.
- Verifies identity / capability / commit / abort / WAL recovery invariants
- across the cross-object transaction surface used by WorldService.

**测试函数** (21 个):

- s01_birth_ab_write_ab_commit
- s02_birth_ab_link_ab_write_a_commit
- s03_birth_ab_write_a_link_ab_commit
- s04_grant_a_to_b_on_c_write_b_commit
- s05_grant_a_to_b_on_c_link_b_c_commit
- s06_grant_then_write_b_link_b_to_a_commit
- s06b_grant_on_c_does_not_authorize_link_to_a
- s07_multi_object_abort_no_residual_state
- s08_grant_then_abort_leaves_no_capability
- s09_grant_commit_wal_recovery_consistent
- s10_grant_abort_wal_recovery_no_residual_cap
- s11_grantor_does_not_become_holder
- s12_grantee_further_grant_semantics
- s_extra_a_three_object_grant_write_link_commit
- s_extra_b_multiple_grants_same_tx
- s_extra_c_abort_then_new_tx_cannot_use_grant
- s_extra_d_grant_commit_new_session_uses_cap
- s_extra_e_consecutive_cross_object_writes_no_drift
- s_extra_f_link_unlink_with_grant
- s_extra_g_multiple_object_switches_before_commit
- s_extra_h_grant_then_switch_identity_and_return

---

### multi_object_transaction_regression.rs

**验证内容**:

- 三个多对象事务回归测试：abort 一致性 / 跨 session capability 隔离 / WAL recovery + link 去重。

**测试函数** (3 个):

- test_a_multi_object_abort_leaves_no_partial_state
- test_b_cross_session_capability_isolation
- test_c_wal_recovery_multi_object_link_no_duplication

---

### object_birth_self_call.rs

**验证内容**:

- 回归测试：OBJECT_BIRTH 不再自动切换身份（enter_object(id) 已删除），
- 但同一事务内，创建者必须能够用 CALL 显式进入自己刚创建的对象。
- 这锁死本轮争论的最终结论：不恢复隐式身份切换，而是让 OBJECT_BIRTH
- 把新对象的 self-AdminCap attach 到 ctx，使 CALL 这条唯一合法的
- 身份切换入口能够真正走通 authorize_intent 审计。
- 特别覆盖 root 身份（current_object == 0）这个此前被认为"拿不到
- 权限"的死结场景：object_birth 对 root 创建者不额外发 grant，
- 但新对象的 self-AdminCap 始终存在且现在会被 attach，
- 所以 root 同样能够 CALL 进它刚创建的对象。

**测试函数** (1 个):

- root_can_call_into_object_it_just_birthed

---

### receipt.rs

**测试函数** (3 个):

- receipt_after_matches_root_hash
- receipt_before_after_consistency
- receipt_replay_consistency

---

### replay.rs

**测试函数** (4 个):

- replay_empty_wal_returns_nonzero
- replay_equals_recovery_idle
- replay_is_deterministic
- replay_different_ops_different_hash

---

### replay_determinism.rs

**测试函数** (4 个):

- same_wal_same_state
- replay_is_deterministic
- object_ops_are_deterministic
- wal_contains_full_world

---

### replay_engine_test.rs

**测试函数** (2 个):

- replay_engine_sees_births
- replay_engine_sees_capability

---

### root_hash.rs

**测试函数** (5 个):

- empty_world_root_hash_is_deterministic
- root_hash_changes_on_write
- root_hash_changes_on_birth
- root_hash_changes_on_link
- root_hash_order_independent

---

### root_link_two_children.rs

**验证内容**:

- 验证: root 身份下连续 birth 两个子对象后，
- 是否可以不经 CALL、直接 OBJECT_LINK 建立两者关系。
- 依据: OBJECT_BIRTH 执行时会把新对象的 self-AdminCap attach 到
- ctx.capabilities（三度修正的最终解法）。这个 attach 是 ctx 级别的
- 全局状态，不区分当前身份是谁。authorize_intent 的 has_pending 分支
- 里 ctx.capabilities.contains(cap_id) 这个 or 条件不要求 grantee
- 等于当前身份，所以理论上 root 不需要 CALL 进子对象就能引用它们
- 已经 attach 的 cap 来完成 LINK。本测试验证这个推断是否成立。

**测试函数** (1 个):

- root_can_link_two_self_birthed_children_without_call

---

### security_recovery_audit.rs

**验证内容**:

- P4: Veritas Security & Recovery Differential Audit
- Focused tests for three open questions from the strength suite:
- 1. WorldService::tx_link vs Machine/Kernel ObjectLink authorization parity
- 2. global_version semantics after WAL recovery (log vs real state)
- 3. Structural WAL attacks (valid UTF-8 / CRC, broken semantics)
- Production code: 0 modifications.
- Principle: prove facts; classify findings; do not fix production here.

**测试函数** (45 个):

- audit_link_worldservice_commit_rejects_without_target_cap
- audit_link_kernel_commit_rejects_without_target_cap
- audit_link_worldservice_succeeds_with_target_cap
- audit_link_authorize_intent_requires_both_endpoints
- audit_link_worldservice_machine_parity
- audit_link_no_preauth_but_commit_gates
- audit_recovery_restores_global_version
- audit_recovery_version_continues_after_restart
- audit_recovery_receipts_since_sees_history
- audit_recovery_occ_baseline_matches_version
- audit_recover_max_version_ignores_txcommitted_but_apply_sets_engine
- audit_wal_duplicate_transaction_committed
- audit_wal_duplicate_birth_id_in_new_tx
- audit_wal_out_of_order_version
- audit_wal_duplicate_link_records
- audit_wal_duplicate_capability_grant
- audit_recovery_commit_recovery_chain
- audit_wal_empty_delta_bumps_version
- audit_wal_illegal_field_values
- audit_wal_corrupt_crc_preserves_prior
- audit_wal_replay_committed_delta_idempotent
- audit_write_cross_object_still_denied
- audit_commit_version_first_apply
- audit_commit_version_consecutive_apply
- audit_equal_version_same_content_is_idempotent
- audit_equal_version_different_content_is_rejected
- audit_stale_version_is_rejected
- audit_version_gap_is_rejected
- audit_repeated_wal_replay_is_idempotent
- audit_rejected_delta_is_atomic
- audit_equal_version_same_content_preserves_root
- audit_canonical_identity_same_delta_equal
- audit_canonical_identity_excludes_tx_id
- audit_canonical_identity_excludes_commit_version
- audit_canonical_identity_includes_actor_id
- audit_canonical_identity_vec_order_matters
- audit_canonical_identity_string_boundary_safe
- audit_canonical_identity_empty_vs_nonempty_vec
- audit_canonical_identity_option_some_none
- audit_canonical_identity_enum_variants
- audit_canonical_identity_every_semantic_field
- audit_last_applied_delta_hash_genesis_is_zero
- audit_last_applied_delta_hash_updates_on_apply
- audit_checkpoint_preserves_last_applied_delta_hash
- audit_checkpoint_roundtrip_identity_continuity

---

### snapshot_restore_roundtrip.rs

**测试函数** (8 个):

- body_serde_state
- body_serde_module_empty_rule
- body_serde_module_with_rule
- cap_graph_roundtrip_empty
- cap_graph_roundtrip_with_grants
- scope_roundtrip_empty
- scope_roundtrip_with_data
- topology_roundtrip_empty

---

### strength_adversarial.rs

**验证内容**:

- Veritas Strength / Adversarial Regression Suite
- Goal: pressure, attack, fault-injection, and boundary tests.
- Principle: only tests. No production-code changes.
- Every failure must be classified: true bug / wrong assumption /
- unsupported / known debt / infra issue.

**测试函数** (37 个):

- s_a01_illegal_grantor
- s_a02_grantor_grantee_swap_foreign_resource
- s_a03_cross_object_write_without_cap
- s_a04_self_access_exemption_boundary
- s_a05_abort_invalidates_pending_capability
- s_a06_revoke_then_use_denied
- s_a07_unauthorized_freeze
- s_a08_unauthorized_death
- s_b01_session_cannot_see_uncommitted_writes
- s_b02_session_cannot_use_other_pending_cap
- s_b03_abort_clears_pending_objects
- s_b04_ended_session_rejected
- s_b05_nonexistent_session
- s_w01_truncated_final_record_no_panic
- s_w02_single_byte_corruption_no_panic
- s_w03_empty_wal
- s_r01_recovery_idempotent
- s_r02_complex_world_recovery_stable
- s_w04_duplicate_wal_line
- s_g01_nonexistent_object_ops
- s_g02_object_id_zero_boundary
- s_h01_empty_and_large_payload
- s_h02_multiple_overwrite_same_state
- s_t01_double_commit
- s_t02_double_abort
- s_t03_commit_then_abort
- s_t04_abort_then_commit
- s_t05_multiple_sessions_allowed
- s_e01_stress_100_objects
- s_e02_stress_many_writes_one_tx
- s_e03_stress_1000_objects
- s_e04_stress_wide_capability_graph
- s_c01_concurrent_different_objects
- s_c02_concurrent_session_lifecycle
- s_c03_concurrent_same_object_writes
- s_a09_link_without_capability
- s_a10_grantor_does_not_become_grantee

---

### wal_recovery_equivalence.rs

**测试函数** (7 个):

- equivalence_single_birth
- equivalence_birth_and_link
- equivalence_full_lifecycle
- equivalence_multi_object_topology
- equivalence_death_cascade
- cross_tx_unlink_then_death_no_cascade
- cross_tx_link_then_death_cascade

---

### wal_recovery_invariants.rs

**测试函数** (4 个):

- recovery_invariant_birth_then_death
- recovery_invariant_birth_freeze_then_death
- recovery_invariant_link_then_unlink
- recovery_invariant_owner_death_removes_link

---

### wal_recovery_object.rs

**测试函数** (3 个):

- object_birth_survives_recovery
- object_link_survives_recovery
- aborted_object_not_recovered

---

### wal_recovery_robustness.rs

**测试函数** (7 个):

- recovery_is_idempotent
- empty_wal_recovery_succeeds
- truncated_wal_last_10_bytes
- truncated_wal_last_50_bytes
- truncated_wal_last_200_bytes
- corrupted_wal_middle_byte
- corrupted_wal_early_byte

---

### world_demo.rs

**验证内容**:

- world_demo: 端到端真实执行 demo。
- birth A -> write A -> birth B -> write B -> link A->B -> 单次 commit -> WAL recovery -> 校验一致
- 通过 WorldService session 完成，同一 session/同一 TransactionContext 内一次性 commit。

**测试函数** (1 个):

- world_demo_multi_object_birth_write_link_commit_recover

---

## 总数

- 测试文件: 34 个
- 测试函数: 202 个
