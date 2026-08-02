#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkType {
    Owns,
    References,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Edge {
    pub id: EdgeId,
    pub from: u64,
    pub to: u64,
    pub kind: LinkType,
}
