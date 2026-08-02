use crate::graph::store::GraphStore;
use crate::graph::types::{Edge, LinkType};
use crate::types::{ObjectId, ObjectState};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphPolicyError {
    CanonicalConflict { proposed_from: u64, proposed_to: u64 },
    NodeDead(u64),
    NodeNotFound(u64),
    OwnsCycleDetected { proposed_from: u64, proposed_to: u64 },
}

pub struct GraphPolicy;

impl GraphPolicy {
    pub fn validate<F>(
        store: &GraphStore,
        proposed: &Edge,
        get_state: F,
    ) -> Result<(), GraphPolicyError>
    where
        F: Fn(ObjectId) -> Option<ObjectState>,
    {
        // 1. Boundary Check
        Self::check_boundary(proposed.from, &get_state)?;
        Self::check_boundary(proposed.to, &get_state)?;

        // 2. Canonical Edge Uniqueness Check
        for existing in store.outgoing_edges(proposed.from) {
            if existing.to == proposed.to && existing.kind == proposed.kind {
                return Err(GraphPolicyError::CanonicalConflict {
                    proposed_from: proposed.from,
                    proposed_to: proposed.to,
                });
            }
        }

        // 3. Owns DAG Topology Check
        if proposed.kind == LinkType::Owns {
            Self::check_owns_dag(store, proposed.from, proposed.to)?;
        }

        Ok(())
    }

    fn check_boundary<F>(node_id: u64, get_state: &F) -> Result<(), GraphPolicyError>
    where
        F: Fn(ObjectId) -> Option<ObjectState>,
    {
        match get_state(node_id) {
            Some(ObjectState::Alive) | Some(ObjectState::Frozen) => Ok(()),
            Some(ObjectState::Dead) => Err(GraphPolicyError::NodeDead(node_id)),
            None => Err(GraphPolicyError::NodeNotFound(node_id)),
        }
    }

    fn check_owns_dag(
        store: &GraphStore,
        proposed_from: u64,
        proposed_to: u64,
    ) -> Result<(), GraphPolicyError> {
        if proposed_from == proposed_to {
            return Err(GraphPolicyError::OwnsCycleDetected {
                proposed_from,
                proposed_to,
            });
        }

        let mut visited = BTreeSet::new();
        let mut stack = vec![proposed_to];

        while let Some(curr) = stack.pop() {
            if curr == proposed_from {
                return Err(GraphPolicyError::OwnsCycleDetected {
                    proposed_from,
                    proposed_to,
                });
            }

            if visited.insert(curr) {
                for edge in store.outgoing_edges(curr) {
                    if edge.kind == LinkType::Owns {
                        stack.push(edge.to);
                    }
                }
            }
        }

        Ok(())
    }
}
