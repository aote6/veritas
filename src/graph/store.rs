use crate::graph::types::{Edge, EdgeId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GraphStore {
    edges: BTreeMap<EdgeId, Edge>,
    outgoing: BTreeMap<u64, BTreeSet<EdgeId>>,
    incoming: BTreeMap<u64, BTreeSet<EdgeId>>,
}

impl GraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_edge(&mut self, edge: Edge) {
        let edge_id = edge.id;
        let from = edge.from;
        let to = edge.to;

        self.outgoing.entry(from).or_default().insert(edge_id);
        self.incoming.entry(to).or_default().insert(edge_id);
        self.edges.insert(edge_id, edge);
    }

    pub fn remove_edge(&mut self, edge_id: EdgeId) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(&edge_id) {
            if let Some(outs) = self.outgoing.get_mut(&edge.from) {
                outs.remove(&edge_id);
            }
            if let Some(ins) = self.incoming.get_mut(&edge.to) {
                ins.remove(&edge_id);
            }
            Some(edge)
        } else {
            None
        }
    }

    pub fn lookup_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.edges.get(&edge_id)
    }

    pub fn contains_edge(&self, edge_id: EdgeId) -> bool {
        self.edges.contains_key(&edge_id)
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    pub fn outgoing_edges(&self, node_id: u64) -> impl Iterator<Item = &Edge> {
        self.outgoing
            .get(&node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.edges.get(id))
    }

    pub fn incoming_edges(&self, node_id: u64) -> impl Iterator<Item = &Edge> {
        self.incoming
            .get(&node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.edges.get(id))
    }

    pub fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }
}
