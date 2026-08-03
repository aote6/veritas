use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use veritas_kernel::engine::VeritasEngine;
use veritas_kernel::types::{ObjectState, LinkType, AbortReason};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
fn unique_wal_path() -> String {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("target/test_prop_{}_{}.wal", std::process::id(), id)
}

// ===== Random operation generator =====

#[derive(Debug, Clone)]
enum Op {
    Birth(u64),
    Freeze(u64),
    Death(u64),
    Link(u64, u64, LinkType),
    Unlink(u64, u64),
    Grant(u64, u64, String),  // grantor, grantee, cap_type
}

struct OpGenerator {
    rng: u64,  // simple LCG
    next_id: u64,
    alive_ids: Vec<u64>,
}

impl OpGenerator {
    fn new(seed: u64) -> Self {
        OpGenerator { rng: seed, next_id: 1, alive_ids: Vec::new() }
    }

    fn rand(&mut self) -> u64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.rng
    }

    fn random_alive(&mut self) -> Option<u64> {
        if self.alive_ids.is_empty() {
            return None;
        }
        let idx = (self.rand() as usize) % self.alive_ids.len();
        Some(self.alive_ids[idx])
    }

    /// Generate a random operation that is valid given current state.
    fn generate(&mut self) -> Op {
        loop {
            let choice = self.rand() % 10;
            match choice {
                0..=3 => {
                    // Birth — always valid
                    let id = self.next_id;
                    self.next_id += 1;
                    self.alive_ids.push(id);
                    return Op::Birth(id);
                }
                4 => {
                    // Freeze — requires an alive object
                    if let Some(id) = self.random_alive() {
                        return Op::Freeze(id);
                    }
                }
                5 => {
                    // Death — requires an alive object
                    if let Some(id) = self.random_alive() {
                        self.alive_ids.retain(|&x| x != id);
                        return Op::Death(id);
                    }
                }
                6..=7 => {
                    // Link — requires two alive objects
                    if self.alive_ids.len() >= 2 {
                        let len = self.alive_ids.len();
                        let i = (self.rand() as usize) % len;
                        let j = (self.rand() as usize) % len;
                        if i != j {
                            let lt = match self.rand() % 3 {
                                0 => LinkType::Owns,
                                1 => LinkType::DependsOn,
                                _ => LinkType::References,
                            };
                            return Op::Link(self.alive_ids[i], self.alive_ids[j], lt);
                        }
                    }
                }
                8 => {
                    // Unlink — requires an alive object
                    if let Some(id) = self.random_alive() {
                        return Op::Unlink(id, id);  // self-unlink always safe-ish
                    }
                }
                _ => {
                    // Grant capability
                    if self.alive_ids.len() >= 2 {
                        let len = self.alive_ids.len();
                        let i = (self.rand() as usize) % len;
                        let j = (self.rand() as usize) % len;
                        let grantor = self.alive_ids[i];
                        let grantee = self.alive_ids[j];
                        let cap = match self.rand() % 3 {
                            0 => "read".to_string(),
                            1 => "write".to_string(),
                            _ => "AdminCap".to_string(),
                        };
                        return Op::Grant(grantor, grantee, cap);
                    }
                }
            }
            // If we couldn't generate a valid op, loop
        }
    }
}

// ===== Snapshot (same as P29.3 but extended) =====

struct EngineSnapshot {
    object_ids: HashSet<u64>,
    object_states: Vec<(u64, ObjectState)>,
    links: Vec<(u64, u64)>,
}

impl EngineSnapshot {
    fn capture(engine: &VeritasEngine) -> Self {
        let object_ids: HashSet<u64> = engine.list_object_ids().into_iter().collect();
        let mut object_states: Vec<(u64, ObjectState)> = object_ids
            .iter()
            .filter_map(|&id| engine.get_object_state(id).map(|s| (id, s)))
            .collect();
        object_states.sort_by_key(|(id, _)| *id);

        let mut links = Vec::new();
        for &from in &object_ids {
            for &to in &object_ids {
                if from != to && engine.has_link(from, to) {
                    links.push((from, to));
                }
            }
        }
        links.sort();

        EngineSnapshot { object_ids, object_states, links }
    }
}

fn execute_op(engine: &VeritasEngine, op: &Op) {
    match op {
        Op::Birth(id) => {
            let mut tx = engine.begin();
            if engine.object_birth(&mut tx, *id).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
        Op::Freeze(id) => {
            let mut tx = engine.begin();
            if engine.object_freeze(&mut tx, *id).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
        Op::Death(id) => {
            let mut tx = engine.begin();
            if engine.object_death(&mut tx, *id).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
        Op::Link(from, to, lt) => {
            let mut tx = engine.begin();
            if engine.object_link(&mut tx, *from, *to, *lt).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
        Op::Unlink(from, to) => {
            let mut tx = engine.begin();
            if engine.object_unlink(&mut tx, *from, *to).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
        Op::Grant(grantor, grantee, cap_type) => {
            let mut tx = engine.begin();
            if engine.capability_grant(&mut tx, *grantee, cap_type, *grantor).is_ok() {
                let _ = engine.commit(&mut tx);
            } else {
                engine.abort(&mut tx, AbortReason::WriteConflict);
            }
        }
    }
}

fn assert_equivalence(ops: &[Op], snapshot_before: &EngineSnapshot, wal_path: &str) {
    let recovered = VeritasEngine::with_wal_path(wal_path.to_string());
    let snapshot_after = EngineSnapshot::capture(&recovered);

    if snapshot_before.object_ids != snapshot_after.object_ids {
        panic!(
            "Recovery mismatch: object_ids differ.\n  ops: {:?}\n  before: {:?}\n  after: {:?}",
            ops,
            snapshot_before.object_ids,
            snapshot_after.object_ids
        );
    }
    if snapshot_before.object_states != snapshot_after.object_states {
        panic!(
            "Recovery mismatch: object_states differ.\n  ops: {:?}\n  before: {:?}\n  after: {:?}",
            ops,
            snapshot_before.object_states,
            snapshot_after.object_states
        );
    }
    if snapshot_before.links != snapshot_after.links {
        panic!(
            "Recovery mismatch: links differ.\n  ops: {:?}\n  before: {:?}\n  after: {:?}",
            ops,
            snapshot_before.links,
            snapshot_after.links
        );
    }
}

fn property_test_run(seed: u64, num_ops: usize) {
    let wal_path = unique_wal_path();
    let _ = std::fs::remove_file(&wal_path);

    let mut generator = OpGenerator::new(seed);
    let mut ops = Vec::new();

    // Generate operations
    for _ in 0..num_ops {
        ops.push(generator.generate());
    }

    let snapshot_before;
    {
        let engine = VeritasEngine::with_wal_path(wal_path.clone());
        for op in &ops {
            execute_op(&engine, op);
        }
        snapshot_before = EngineSnapshot::capture(&engine);
    } // crash

    assert_equivalence(&ops, &snapshot_before, &wal_path);

    let _ = std::fs::remove_file(&wal_path);
}

// ===== Tests =====

/// P29.4: 10 rounds of random 10-op sequences, different seeds.
#[test]
fn property_10_rounds_10_ops() {
    for round in 0..10 {
        property_test_run(42 + round * 100, 10);
    }
}

/// P29.4: 10 rounds of random 50-op sequences.
#[test]
fn property_10_rounds_50_ops() {
    for round in 0..10 {
        property_test_run(1000 + round * 200, 50);
    }
}

/// P29.4: 5 rounds of random 200-op sequences (stress).
#[test]
fn property_5_rounds_200_ops() {
    for round in 0..5 {
        property_test_run(5000 + round * 500, 200);
    }
}
