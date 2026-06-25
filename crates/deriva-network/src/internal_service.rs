use std::collections::BTreeMap;
use std::sync::Arc;

use tonic::{Request, Response, Status};

use deriva_core::address::{CAddr, FunctionId, Recipe, Value};
use deriva_core::persistent_dag::PersistentDag;
use deriva_storage::SledRecipeStore;

use crate::anti_entropy::{compute_recipe_fingerprint, verify_recipe_addr};
use crate::proto::deriva_internal_server::DerivaInternal;
use crate::proto::*;

/// Internal gRPC service for node-to-node recipe replication and sync.
///
/// Handles incoming ReplicateRecipe, SyncRecipes, and FetchRecipes RPCs
/// from cluster peers. NOT exposed to external clients.
pub struct InternalService {
    dag: Arc<PersistentDag>,
    recipes: Arc<SledRecipeStore>,
}

impl InternalService {
    pub fn new(dag: Arc<PersistentDag>, recipes: Arc<SledRecipeStore>) -> Self {
        Self { dag, recipes }
    }
}

fn parse_addr(bytes: &[u8]) -> Result<CAddr, Status> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("addr must be 32 bytes"))?;
    Ok(CAddr::from_raw(arr))
}


#[tonic::async_trait]
impl DerivaInternal for InternalService {
    /// Handle incoming recipe replication from a peer coordinator.
    ///
    /// 1. Parse and reconstruct the Recipe from request fields
    /// 2. Verify CAddr integrity (recompute and compare)
    /// 3. Insert into local DAG + recipe store
    /// 4. Return success (idempotent: already-exists is also success)
    async fn replicate_recipe(
        &self,
        request: Request<ReplicateRecipeRequest>,
    ) -> Result<Response<ReplicateRecipeResponse>, Status> {
        let req = request.into_inner();

        let claimed_addr = parse_addr(&req.addr)?;

        let inputs: Vec<CAddr> = req
            .inputs
            .iter()
            .map(|b| parse_addr(b))
            .collect::<Result<_, _>>()?;

        let params: BTreeMap<String, Value> = req
            .params
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();

        let recipe = Recipe::new(
            FunctionId::new(&req.function_name, &req.function_version),
            inputs,
            params,
        );

        // CAddr verification (FR-3.1–FR-3.4)
        if !verify_recipe_addr(&claimed_addr, &recipe) {
            let computed = recipe.addr();
            tracing::error!(
                claimed = %claimed_addr,
                computed = %computed,
                "replicate_recipe: CAddr verification failed — rejecting"
            );
            return Ok(Response::new(ReplicateRecipeResponse {
                success: false,
                error: format!(
                    "CAddr mismatch: claimed {} but computed {}",
                    claimed_addr, computed
                ),
            }));
        }

        // Insert into DAG (idempotent — already-exists is fine)
        match self.dag.insert(&recipe) {
            Ok(_) => {}
            Err(deriva_core::error::DerivaError::CycleDetected(msg)) => {
                return Ok(Response::new(ReplicateRecipeResponse {
                    success: false,
                    error: format!("cycle detected: {}", msg),
                }));
            }
            Err(e) => {
                return Ok(Response::new(ReplicateRecipeResponse {
                    success: false,
                    error: format!("DAG insert failed: {}", e),
                }));
            }
        }

        // Insert into recipe store (idempotent)
        if let Err(e) = self.recipes.put(&claimed_addr, &recipe) {
            tracing::warn!(addr = %claimed_addr, error = %e, "recipe store put failed");
            // DAG insert succeeded — recipe is still usable, just not persisted in store
        }

        Ok(Response::new(ReplicateRecipeResponse {
            success: true,
            error: String::new(),
        }))
    }


    /// Handle anti-entropy fingerprint exchange.
    ///
    /// Computes local recipe fingerprint, compares to peer's fingerprint.
    /// If equal → in_sync=true (no further action needed).
    /// If different → returns full list of local recipe addrs for diff computation.
    async fn sync_recipes(
        &self,
        request: Request<SyncRecipesRequest>,
    ) -> Result<Response<SyncRecipesResponse>, Status> {
        let req = request.into_inner();

        let local_addrs = self.dag.all_addrs();
        let local_fp = compute_recipe_fingerprint(&local_addrs);

        let in_sync = local_fp == req.fingerprint;

        let recipe_addrs = if in_sync {
            vec![]
        } else {
            local_addrs.iter().map(|a| a.as_bytes().to_vec()).collect()
        };

        Ok(Response::new(SyncRecipesResponse {
            in_sync,
            fingerprint: local_fp,
            recipe_addrs,
        }))
    }

    /// Handle fetch recipes request — return full recipe data for requested addrs.
    ///
    /// Looks up each requested CAddr in the recipe store.
    /// Missing recipes are silently skipped (may have been GC'd).
    async fn fetch_recipes(
        &self,
        request: Request<FetchRecipesRequest>,
    ) -> Result<Response<FetchRecipesResponse>, Status> {
        let req = request.into_inner();

        let mut recipes = Vec::with_capacity(req.addrs.len());

        for addr_bytes in &req.addrs {
            let addr = match parse_addr(addr_bytes) {
                Ok(a) => a,
                Err(_) => continue, // skip invalid addrs
            };

            let recipe = match self.recipes.get(&addr) {
                Ok(Some(r)) => r,
                Ok(None) => continue, // skip missing (may be GC'd)
                Err(_) => continue,   // skip on storage error
            };

            recipes.push(RecipeData {
                addr: addr_bytes.clone(),
                function_name: recipe.function_id.name.clone(),
                function_version: recipe.function_id.version.clone(),
                inputs: recipe.inputs.iter().map(|a| a.as_bytes().to_vec()).collect(),
                params: recipe
                    .params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            });
        }

        Ok(Response::new(FetchRecipesResponse { recipes }))
    }
}
