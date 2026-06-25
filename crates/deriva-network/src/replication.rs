use std::net::SocketAddr;
use std::time::Duration;

use tokio::task::JoinSet;
use tonic::transport::Channel;

use deriva_core::address::{CAddr, Recipe};
use deriva_core::error::DerivaError;

use crate::proto::deriva_internal_client::DerivaInternalClient;
use crate::proto::ReplicateRecipeRequest;
use crate::types::NodeId;

/// Configuration for recipe replication.
#[derive(Debug, Clone)]
pub struct ReplicationConfig {
    /// Timeout for each replication RPC.
    pub rpc_timeout: Duration,
    /// Max retries per peer on failure.
    pub max_retries: u32,
    /// Anti-entropy sync interval.
    pub sync_interval: Duration,
    /// Whether to require ALL peers to ACK for put_recipe to succeed.
    pub require_all: bool,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            rpc_timeout: Duration::from_secs(5),
            max_retries: 3,
            sync_interval: Duration::from_secs(30),
            require_all: true,
        }
    }
}


/// Result of a replication attempt to all peers.
#[derive(Debug)]
pub struct ReplicationResult {
    /// Number of peers that successfully acknowledged.
    pub success_count: usize,
    /// Number of peers that failed after all retries.
    pub failure_count: usize,
    /// Total number of peers targeted.
    pub total_peers: usize,
    /// Details of each failure: (peer identity, error message).
    pub failures: Vec<(NodeId, String)>,
}

impl ReplicationResult {
    /// Returns true if all peers acknowledged successfully.
    pub fn is_fully_replicated(&self) -> bool {
        self.failure_count == 0
    }
}

/// Handles synchronous recipe replication to cluster peers.
pub struct RecipeReplicator {
    config: ReplicationConfig,
}

impl RecipeReplicator {
    pub fn new(config: ReplicationConfig) -> Self {
        Self { config }
    }

    /// Replicate a recipe to all specified peers concurrently.
    ///
    /// Sends ReplicateRecipe RPC to each peer in parallel using a JoinSet.
    /// Retries failed peers up to `max_retries` times with exponential backoff.
    /// Returns a ReplicationResult summarizing successes and failures.
    pub async fn replicate_to_all(
        &self,
        recipe: &Recipe,
        addr: &CAddr,
        peers: &[(NodeId, SocketAddr)],
    ) -> ReplicationResult {
        if peers.is_empty() {
            return ReplicationResult {
                success_count: 0,
                failure_count: 0,
                total_peers: 0,
                failures: vec![],
            };
        }

        let mut join_set = JoinSet::new();

        for (peer_id, grpc_addr) in peers {
            let peer_id = peer_id.clone();
            let grpc_addr = *grpc_addr;
            let addr = *addr;
            let recipe = recipe.clone();
            let timeout = self.config.rpc_timeout;
            let max_retries = self.config.max_retries;

            join_set.spawn(async move {
                let mut last_err = String::new();
                for attempt in 0..=max_retries {
                    match Self::send_replicate(&grpc_addr, &addr, &recipe, timeout).await {
                        Ok(()) => return (peer_id, Ok(())),
                        Err(e) => {
                            last_err = e.to_string();
                            if attempt < max_retries {
                                // Exponential backoff: 100ms, 200ms, 300ms, ...
                                let backoff = Duration::from_millis(100 * (attempt as u64 + 1));
                                tokio::time::sleep(backoff).await;
                            }
                        }
                    }
                }
                (peer_id, Err(last_err))
            });
        }

        let mut success_count = 0usize;
        let mut failures = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((_, Ok(()))) => success_count += 1,
                Ok((peer_id, Err(e))) => failures.push((peer_id, e)),
                Err(join_err) => {
                    failures.push((
                        NodeId::new("0.0.0.0:0".parse().unwrap(), "unknown"),
                        format!("task join error: {}", join_err),
                    ));
                }
            }
        }

        ReplicationResult {
            success_count,
            failure_count: failures.len(),
            total_peers: peers.len(),
            failures,
        }
    }


    /// Send a single ReplicateRecipe RPC to one peer.
    ///
    /// Connects to the peer's gRPC endpoint, serializes the recipe,
    /// sends the request with a timeout, and interprets the response.
    async fn send_replicate(
        grpc_addr: &SocketAddr,
        addr: &CAddr,
        recipe: &Recipe,
        timeout: Duration,
    ) -> Result<(), DerivaError> {
        let endpoint = format!("http://{}", grpc_addr);
        let channel = Channel::from_shared(endpoint)
            .map_err(|e| DerivaError::Storage(format!("invalid endpoint: {}", e)))?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect()
            .await
            .map_err(|e| DerivaError::Storage(format!("connect failed: {}", e)))?;

        let mut client = DerivaInternalClient::new(channel);

        let request = tonic::Request::new(ReplicateRecipeRequest {
            addr: addr.as_bytes().to_vec(),
            function_name: recipe.function_id.name.clone(),
            function_version: recipe.function_id.version.clone(),
            inputs: recipe.inputs.iter().map(|a| a.as_bytes().to_vec()).collect(),
            params: recipe.params.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
        });

        let response = tokio::time::timeout(timeout, client.replicate_recipe(request))
            .await
            .map_err(|_| DerivaError::Storage("replication RPC timeout".into()))?
            .map_err(|e| DerivaError::Storage(format!("replication RPC error: {}", e)))?;

        let resp = response.into_inner();
        if resp.success {
            Ok(())
        } else {
            Err(DerivaError::Storage(format!("peer rejected recipe: {}", resp.error)))
        }
    }
}
