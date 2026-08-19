//! Host Call unified enumeration (constitution kernel.md §6).
//!
//! Host Calls are provided by the external environment, not by Kernel mode.
//! This module is intentionally standalone — no dependency on other Veritas modules.

/// Host Call identifiers exposed via `Instruction::HostCall { call_id }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCall {
    Time,
    Random,
    Write,
    Read,
    Spawn,
}

impl HostCall {
    /// Map a raw call_id (u8) to a HostCall variant.
    /// Returns `None` for unknown ids.
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(HostCall::Time),
            1 => Some(HostCall::Random),
            2 => Some(HostCall::Write),
            3 => Some(HostCall::Read),
            4 => Some(HostCall::Spawn),
            _ => None,
        }
    }

    /// Stable numeric id for this HostCall.
    pub fn to_id(&self) -> u8 {
        match self {
            HostCall::Time => 0,
            HostCall::Random => 1,
            HostCall::Write => 2,
            HostCall::Read => 3,
            HostCall::Spawn => 4,
        }
    }

    /// Human-readable name (for diagnostics / tracing).
    pub fn name(&self) -> &'static str {
        match self {
            HostCall::Time => "host_time",
            HostCall::Random => "host_random",
            HostCall::Write => "host_write",
            HostCall::Read => "host_read",
            HostCall::Spawn => "host_spawn",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_id_time() {
        assert_eq!(HostCall::from_id(0), Some(HostCall::Time));
    }

    #[test]
    fn from_id_spawn() {
        assert_eq!(HostCall::from_id(4), Some(HostCall::Spawn));
    }

    #[test]
    fn from_id_invalid() {
        assert_eq!(HostCall::from_id(5), None);
    }

    #[test]
    fn to_id_roundtrip() {
        for id in 0u8..=4 {
            let hc = HostCall::from_id(id).unwrap();
            assert_eq!(hc.to_id(), id);
            assert!(!hc.name().is_empty());
        }
    }
}
