pub mod anti_entropy;
pub mod bloom;
pub mod config;
pub mod memberlist;
pub mod message;
pub mod metrics;
pub mod replication;
pub mod runtime;
pub mod types;

pub mod proto {
    tonic::include_proto!("deriva_internal");
}

pub use config::SwimConfig;
pub use runtime::SwimRuntime;
pub use types::{MemberState, NodeId, NodeMetadata, NodeRole, SwimEvent};
