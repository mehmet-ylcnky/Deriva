use crate::types::{MemberUpdate, NodeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwimMessage {
    Ping { sender: NodeId, sequence: u64, piggyback: Vec<MemberUpdate> },
    Ack { sender: NodeId, sequence: u64, piggyback: Vec<MemberUpdate> },
    PingReq { sender: NodeId, target: NodeId, sequence: u64, piggyback: Vec<MemberUpdate> },
    Sync { sender: NodeId, members: Vec<MemberUpdate> },
}

impl SwimMessage {
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Encode with UDP size limit (65507 bytes). Returns None if too large.
    pub fn encode_checked(&self) -> Option<Vec<u8>> {
        let data = bincode::serialize(self).ok()?;
        if data.len() > 65507 { None } else { Some(data) }
    }

    pub fn decode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    pub fn piggyback(&self) -> &[MemberUpdate] {
        match self {
            Self::Ping { piggyback, .. } => piggyback,
            Self::Ack { piggyback, .. } => piggyback,
            Self::PingReq { piggyback, .. } => piggyback,
            Self::Sync { members, .. } => members,
        }
    }

    pub fn sender(&self) -> &NodeId {
        match self {
            Self::Ping { sender, .. }
            | Self::Ack { sender, .. }
            | Self::PingReq { sender, .. }
            | Self::Sync { sender, .. } => sender,
        }
    }
}
