use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tonic::transport::Channel;

use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::error::DerivaError;
use deriva_core::persistent_dag::PersistentDag;
use deriva_storage::SledRecipeStore;

use crate::proto::deriva_internal_client::DerivaInternalClient;
use crate::proto::{FetchRecipesRequest, SyncRecipesRequest};
use crate::replication::ReplicationConfig;
use crate::runtime::SwimRuntime;
use crate::types::MemberState;

/// Compute a fingerprint of a set of recipe CAddrs.
///
/// Fingerprint = SHA-256(sorted concatenation of all 32-byte CAddr values).
/// Two nodes with identical recipe sets produce identical fingerprints
/// regardless of insertion order.
pub fn compute_recipe_fingerprint(addrs: &[CAddr]) -> Vec<u8> {
    let mut sorted: Vec<CAddr> = addrs.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    for addr in &sorted {
        hasher.update(addr.as_bytes());
    }
    hasher.finalize().to_vec()
}


/// Find recipes that exist in `remote_addrs` but not in `local_addrs`.
///
/// Returns the set difference: remote \ local.
pub fn find_missing_recipes(local_addrs: &[CAddr], remote_addrs: &[CAddr]) -> Vec<CAddr> {
    let local_set: HashSet<CAddr> = local_addrs.iter().copied().collect();
    remote_addrs
        .iter()
        .filter(|addr| !local_set.contains(addr))
        .copied()
        .collect()
}

/// Verify that a recipe's claimed CAddr matches its computed CAddr.
///
/// Recomputes the recipe address from its fields (function_id, inputs, params)
/// and compares to the claimed address. Returns false if they don't match,
/// indicating a corrupt or tampered recipe.
pub fn verify_recipe_addr(claimed: &CAddr, recipe: &Recipe) -> bool {
    recipe.addr() == *claimed
}


/// Background anti-entropy sync loop.
///
/// Periodically picks a random alive peer and synchronizes recipe sets:
/// 1. Compute local fingerprint
/// 2. Exchange fingerprints with peer via SyncRecipes RPC
/// 3. If fingerprints match → in sync, no work
/// 4. If different → compute missing recipes, fetch them, verify + insert
///
/// Handles failures gracefully: unreachable peers are skipped,
/// CAddr mismatches are logged and rejected.
pub async fn anti_entropy_loop(
    dag: Arc<PersistentDag>,
    recipes: Arc<SledRecipeStore>,
    swim: Arc<SwimRuntime>,
    config: ReplicationConfig,
) {
    let mut interval = tokio::time::interval(config.sync_interval);
    loop {
        interval.tick().await;

        if let Err(e) = run_sync_round(&dag, &recipes, &swim, &config).await {
            tracing::debug!(error = %e, "anti-entropy sync round failed");
        }
    }
}

/// Execute a single anti-entropy sync round with a random peer.
async fn run_sync_round(
    dag: &PersistentDag,
    recipes: &SledRecipeStore,
    swim: &SwimRuntime,
    config: &ReplicationConfig,
) -> Result<(), DerivaError> {
    // Step 1: Pick a random alive peer with gRPC address
    let peer_grpc = pick_random_peer_grpc(swim).await
        .ok_or_else(|| DerivaError::Storage("no alive peers for sync".into()))?;

    // Step 2: Compute local fingerprint
    let local_addrs = dag.all_addrs();
    let local_fp = compute_recipe_fingerprint(&local_addrs);

    // Step 3: Connect to peer and exchange fingerprints
    let channel = Channel::from_shared(format!("http://{}", peer_grpc))
        .map_err(|e| DerivaError::Storage(format!("invalid endpoint: {}", e)))?
        .connect_timeout(config.rpc_timeout)
        .timeout(config.rpc_timeout)
        .connect()
        .await
        .map_err(|e| DerivaError::Storage(format!("sync connect failed: {}", e)))?;

    let mut client = DerivaInternalClient::new(channel);

    let sync_resp = client
        .sync_recipes(tonic::Request::new(SyncRecipesRequest {
            fingerprint: local_fp,
            recipe_count: local_addrs.len() as u64,
        }))
        .await
        .map_err(|e| DerivaError::Storage(format!("sync RPC failed: {}", e)))?
        .into_inner();

    // Step 4: If in sync, nothing to do
    if sync_resp.in_sync {
        tracing::debug!("anti-entropy: in sync with peer");
        return Ok(());
    }


    // Step 5: Compute missing recipes (peer has, we don't)
    let remote_addrs: Vec<CAddr> = sync_resp
        .recipe_addrs
        .iter()
        .filter_map(|bytes| {
            <[u8; 32]>::try_from(bytes.as_slice()).ok().map(CAddr::from_raw)
        })
        .collect();

    let missing = find_missing_recipes(&local_addrs, &remote_addrs);
    if missing.is_empty() {
        tracing::debug!("anti-entropy: fingerprints differ but no missing recipes");
        return Ok(());
    }

    tracing::info!(
        missing_count = missing.len(),
        "anti-entropy: fetching missing recipes from peer"
    );

    // Step 6: Fetch missing recipes
    let fetch_resp = client
        .fetch_recipes(tonic::Request::new(FetchRecipesRequest {
            addrs: missing.iter().map(|a| a.as_bytes().to_vec()).collect(),
        }))
        .await
        .map_err(|e| DerivaError::Storage(format!("fetch RPC failed: {}", e)))?
        .into_inner();

    // Step 7: Verify and insert each fetched recipe
    let mut inserted = 0u64;
    let mut rejected = 0u64;

    for recipe_data in &fetch_resp.recipes {
        let claimed_addr = match <[u8; 32]>::try_from(recipe_data.addr.as_slice()) {
            Ok(arr) => CAddr::from_raw(arr),
            Err(_) => {
                rejected += 1;
                continue;
            }
        };

        let inputs: Vec<CAddr> = recipe_data
            .inputs
            .iter()
            .filter_map(|b| <[u8; 32]>::try_from(b.as_slice()).ok().map(CAddr::from_raw))
            .collect();

        let params = recipe_data
            .params
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();

        let recipe = Recipe::new(
            FunctionId::new(&recipe_data.function_name, &recipe_data.function_version),
            inputs,
            params,
        );

        // CAddr verification (FR-3.1–FR-3.4)
        if !verify_recipe_addr(&claimed_addr, &recipe) {
            tracing::error!(
                claimed = %claimed_addr,
                computed = %recipe.addr(),
                "anti-entropy: CAddr verification failed — rejecting recipe"
            );
            rejected += 1;
            continue;
        }

        // Insert into local DAG + recipe store
        match dag.insert(&recipe) {
            Ok(_) => {
                let _ = recipes.put(&claimed_addr, &recipe);
                inserted += 1;
            }
            Err(e) => {
                tracing::debug!(addr = %claimed_addr, error = %e, "anti-entropy: insert failed");
            }
        }
    }

    tracing::info!(
        inserted = inserted,
        rejected = rejected,
        "anti-entropy: sync round complete"
    );

    Ok(())
}

/// Pick a random alive peer and return its gRPC address.
async fn pick_random_peer_grpc(swim: &SwimRuntime) -> Option<SocketAddr> {
    let members = swim.members().await;
    let alive_peers: Vec<_> = members
        .iter()
        .filter(|(id, state)| {
            *state == MemberState::Alive && id.addr != swim.local_id().addr
        })
        .collect();

    if alive_peers.is_empty() {
        return None;
    }

    use rand::Rng;
    let idx = rand::thread_rng().gen_range(0..alive_peers.len());
    let (peer_id, _) = &alive_peers[idx];

    swim.get_metadata(peer_id).await.map(|m| m.grpc_addr)
}
