//! Snapshot restore roundtrip：Body/CapGraph/Scope/Topology 序列化往返。
//!
//! 验证内容：各组件 empty 与有数据情况下的 serde roundtrip 正确。
//! 对应 VERIFICATION_MAP：snapshot_restore_roundtrip.rs
//! 若失败，意味着 snapshot 格式或 serde 实现丢失信息。

// PR3: 第一批 snapshot/restore 可逆性单元测试
// 只测试已有公开 API 的组件：ObjectBody serde, CapabilityGraph, ScopeRegistry, Topology(空)
// ObjectRegistry/StateStore 属于集成测试，等 Engine Checkpoint 接通后再补

use veritas_kernel::types::*;

// ========== 1. ObjectBody serialize/deserialize ==========

/// Body state 组件 serde roundtrip 正确。
/// 失败意味着 state 序列化丢失。
#[test]
fn body_serde_state() {
    let body = ObjectBody::State;
    let bytes = veritas_kernel::engine::VeritasEngine::serialize_object_body(&body);
    let restored = veritas_kernel::engine::VeritasEngine::deserialize_object_body(&bytes);
    assert_eq!(body, restored);
}

/// Body module 空 rule serde roundtrip。
/// 失败意味着 module 空规则序列化错误。
#[test]
fn body_serde_module_empty_rule() {
    let body = ObjectBody::Module {
        code_section: vec![0xAB, 0xCD],
        import_section: vec![10, 20],
        export_section: std::collections::HashMap::from([("init".to_string(), 0)]),
        verification_rule: None,
    };
    let bytes = veritas_kernel::engine::VeritasEngine::serialize_object_body(&body);
    let restored = veritas_kernel::engine::VeritasEngine::deserialize_object_body(&bytes);
    assert_eq!(body, restored);
}

/// Body module 有 rule 时 serde roundtrip。
/// 失败意味着 rule 序列化丢失。
#[test]
fn body_serde_module_with_rule() {
    let body = ObjectBody::Module {
        code_section: vec![0x01, 0x02, 0x03],
        import_section: vec![],
        export_section: std::collections::HashMap::new(),
        verification_rule: Some(VerificationRule {
            max_instances: Some(5),
            allow_instructions: vec![1, 2, 3, 4],
        }),
    };
    let bytes = veritas_kernel::engine::VeritasEngine::serialize_object_body(&body);
    let restored = veritas_kernel::engine::VeritasEngine::deserialize_object_body(&bytes);
    assert_eq!(body, restored);
}

// ========== 2. CapabilityGraph roundtrip ==========

/// 空 CapGraph serde roundtrip。
/// 失败意味着空 capability graph 序列化错误。
#[test]
fn cap_graph_roundtrip_empty() {
    let mut graph = veritas_kernel::capability::CapabilityGraph::new();
    let a = graph.snapshot_capabilities();
    graph.restore_capabilities(&a);
    let b = graph.snapshot_capabilities();
    assert_eq!(a, b);
}

/// 有 grants 的 CapGraph serde roundtrip。
/// 失败意味着 grant 信息在序列化中丢失。
#[test]
fn cap_graph_roundtrip_with_grants() {
    let mut graph = veritas_kernel::capability::CapabilityGraph::new();
    graph.grant_with_sequence("execute".to_string(), 1, 2, 100, 1);
    graph.grant_with_sequence("read".to_string(), 2, 3, 200, 2);
    let a = graph.snapshot_capabilities();
    graph.restore_capabilities(&a);
    let b = graph.snapshot_capabilities();
    assert_eq!(a, b);
}

// ========== 3. ScopeRegistry roundtrip ==========

/// 空 Scope serde roundtrip。
/// 失败意味着空 scope 序列化错误。
#[test]
fn scope_roundtrip_empty() {
    let registry = veritas_kernel::scope_registry::ScopeRegistry::new();
    let a = registry.snapshot_all_scopes();
    registry.restore_scopes(&a);
    let b = registry.snapshot_all_scopes();
    assert_eq!(a, b);
}

/// 有数据的 Scope serde roundtrip。
/// 失败意味着 scope 数据丢失。
#[test]
fn scope_roundtrip_with_data() {
    let registry = veritas_kernel::scope_registry::ScopeRegistry::new();
    registry.declare(1, 100);
    registry.apply_bind(1, 10);
    registry.apply_bind(1, 20);
    registry.declare(2, 200);
    registry.apply_bind(2, 30);
    let a = registry.snapshot_all_scopes();
    registry.restore_scopes(&a);
    let b = registry.snapshot_all_scopes();
    assert_eq!(a, b);
}

// ========== 4. Topology roundtrip (仅空) ==========

/// 空 Topology serde roundtrip。
/// 失败意味着空拓扑序列化错误。
#[test]
fn topology_roundtrip_empty() {
    let engine = veritas_kernel::test_api::empty_engine();
    let a = engine.snapshot_links();
    engine.restore_links(&a);
    let b = engine.snapshot_links();
    assert_eq!(a, b);
}
