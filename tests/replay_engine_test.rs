use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectState, LinkType};
use std::collections::HashSet;

/// P30.2: After executing operations on Engine A, the WAL entries
/// contain enough information to reconstruct the full Engine state
/// (objects, links, capabilities) — not just StateMemory writes.
///
/// This test verifies that Engine::with_wal_path() successfully
/// rebuilds the object_registry and topology from WAL entries.
/// This is the "replay" path that ReplayEngine must eventually own.

#[test]
fn wal_contains_full_world_state() {
    let wal_path = format!("target/test_wal_full_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    // Execute: create objects, links, freeze, death
    let expected_alive: HashSet<u64>;
    let expected_dead: HashSet<u64>;
    let expected_links: Vec<(u64, u64)>;
    let expected_frozen: HashSet<u64>;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());

        // Birth
        for id in [1u64, 2, 3, 10, 20] {
            let mut tx = engine.begin();
            engine.object_birth(&mut tx, id).unwrap();
            engine.commit(&mut tx).unwrap();
        }

        // Links
        let mut tx = engine.begin();
        engine.object_link(&mut tx, 1, 2, LinkType::Owns).unwrap();
        engine.object_link(&mut tx, 1, 3, LinkType::DependsOn).unwrap();
        engine.object_link(&mut tx, 10, 20, LinkType::References).unwrap();
        engine.commit(&mut tx).unwrap();

        // Freeze
        let mut tx = engine.begin();
        engine.object_freeze(&mut tx, 20).unwrap();
        engine.commit(&mut tx).unwrap();

        // Death
        let mut tx = engine.begin();
        engine.object_death(&mut tx, 1).unwrap(); // cascade kills 2, 3
        engine.commit(&mut tx).unwrap();

        // 1 OWNS 2 → 2 cascades to Dead
        // 1 DEPENDS_ON 3 → 3 receives DependencyInvalidated but stays Alive
        expected_dead = [1, 2].iter().cloned().collect();
        expected_alive = [3, 10].iter().cloned().collect();
        expected_frozen = [20].iter().cloned().collect();
        // 10 REFERENCES 20, both alive → link survives
        // 1 OWNS 2 → both dead, link removed
        // 1 DEPENDS_ON 3 → 1 dead, link removed
        expected_links = vec![(10, 20)];
    }

    // Recover — this exercises the WAL→object_registry+topology rebuild path
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());

        for id in &expected_alive {
            assert_eq!(
                engine.get_object_state(*id),
                Some(ObjectState::Alive),
                "object {} should be alive", id
            );
        }
        for id in &expected_dead {
            assert!(engine.is_object_dead(*id), "object {} should be dead", id);
        }
        for id in &expected_frozen {
            assert_eq!(
                engine.get_object_state(*id),
                Some(ObjectState::Frozen),
                "object {} should be frozen", id
            );
        }

        // Verify links
        let alive_ids: Vec<u64> = engine.list_object_ids();
        for &from in &alive_ids {
            for &to in &alive_ids {
                let has_link = engine.has_link(from, to);
                let expected = expected_links.contains(&(from, to));
                assert_eq!(
                    has_link, expected,
                    "link {}-{}: expected {}, got {}", from, to, expected, has_link
                );
            }
        }
    }

    let _ = std::fs::remove_file(&wal_path);
}

/// P30.3: Capability Identity Replay
/// Verifies that grant_sequence is persisted in WAL and restored on recovery,
/// P30.3: Strict Capability Identity & Sequence Replay
/// Verifies that capability_id and grant_sequence are durable WAL first-class state,
/// ensuring exact identity and sequence counter alignment across recovery.
#[test]
fn capability_strict_identity_and_sequence_survives_recovery() {
    let wal_path = format!("target/test_cap_strict_{}.wal", std::process::id());
    let _ = std::fs::remove_file(&wal_path);

    let owner: u64 = 100;
    let holder: u64 = 200;
    let cap_id_1: u64;
    let cap_id_2: u64;
    let seq_before: u64;

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        let mut tx = engine.begin();
        engine.object_birth(&mut tx, owner).unwrap();
        engine.object_birth(&mut tx, holder).unwrap();
        engine.commit(&mut tx).unwrap();

        let mut tx2 = engine.begin();
        cap_id_1 = engine.capability_grant(&mut tx2, holder, "read", owner).unwrap();
        cap_id_2 = engine.capability_grant(&mut tx2, holder, "write", owner).unwrap();
        engine.commit(&mut tx2).unwrap();

        seq_before = engine.capability_sequence();
        assert!(engine.holds_capability(cap_id_1, holder));
        assert!(engine.holds_capability(cap_id_2, holder));
    }

    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        
        // 1. Identity Exact Match Check
        assert!(
            engine.holds_capability(cap_id_1, holder),
            "cap_id_1 ({}) must survive with exact identity match", cap_id_1
        );
        assert!(
            engine.holds_capability(cap_id_2, holder),
            "cap_id_2 ({}) must survive with exact identity match", cap_id_2
        );

        // 2. Monotonic Sequence Alignment Check
        assert_eq!(
            engine.capability_sequence(),
            seq_before,
            "capability_sequence after recovery ({}) must exactly match before recovery ({})",
            engine.capability_sequence(),
            seq_before
        );
    }

    let _ = std::fs::remove_file(&wal_path);
}
