use crate::state::ServerState;
use deriva_core::address::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub mod proto {
    tonic::include_proto!("deriva");
}

use proto::deriva_server::Deriva;
use proto::*;

const CHUNK_SIZE: usize = 64 * 1024;

pub struct DerivaService {
    state: Arc<ServerState>,
}

impl DerivaService {
    pub fn new(state: Arc<ServerState>) -> Self {
        Self { state }
    }
}

#[allow(clippy::result_large_err)]
fn parse_addr(bytes: &[u8]) -> Result<CAddr, Status> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("addr must be 32 bytes"))?;
    Ok(CAddr::from_raw(arr))
}

fn parse_params(params: &std::collections::HashMap<String, String>) -> BTreeMap<String, Value> {
    params
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect()
}

#[tonic::async_trait]
impl Deriva for DerivaService {
    async fn put_leaf(
        &self,
        request: Request<PutLeafRequest>,
    ) -> Result<Response<PutLeafResponse>, Status> {
        let addr = self
            .state
            .storage
            .put_leaf(&request.get_ref().data)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(PutLeafResponse {
            addr: addr.as_bytes().to_vec(),
        }))
    }

    async fn put_recipe(
        &self,
        request: Request<PutRecipeRequest>,
    ) -> Result<Response<PutRecipeResponse>, Status> {
        let req = request.get_ref();
        let inputs: Vec<CAddr> = req
            .inputs
            .iter()
            .map(|b| parse_addr(b))
            .collect::<Result<_, _>>()?;
        let recipe = Recipe::new(
            FunctionId::new(&req.function_name, &req.function_version),
            inputs,
            parse_params(&req.params),
        );
        let addr = self
            .state
            .storage
            .put_recipe(&recipe)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(PutRecipeResponse {
            addr: addr.as_bytes().to_vec(),
        }))
    }

    type GetStream = ReceiverStream<Result<GetResponse, Status>>;

    async fn get(
        &self,
        request: Request<GetRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let state = Arc::clone(&self.state);
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            let result = state.executor.materialize(addr).await;
            match result {
                Ok(data) => {
                    for chunk in data.chunks(CHUNK_SIZE) {
                        if tx
                            .send(Ok(GetResponse {
                                chunk: chunk.to_vec(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn resolve(
        &self,
        request: Request<ResolveRequest>,
    ) -> Result<Response<ResolveResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        match self.state.recipes.get(&addr)
            .map_err(|e| Status::internal(e.to_string()))? {
            Some(recipe) => Ok(Response::new(ResolveResponse {
                found: true,
                function_name: recipe.function_id.name.clone(),
                function_version: recipe.function_id.version.clone(),
                inputs: recipe.inputs.iter().map(|a| a.as_bytes().to_vec()).collect(),
                params: recipe
                    .params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            })),
            None => Ok(Response::new(ResolveResponse::default())),
        }
    }

    async fn invalidate(
        &self,
        request: Request<InvalidateRequest>,
    ) -> Result<Response<InvalidateResponse>, Status> {
        let addr = parse_addr(&request.get_ref().addr)?;
        let was_cached = self.state.cache.remove(&addr).await.is_some();
        Ok(Response::new(InvalidateResponse { was_cached }))
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let stats = &self.state.executor.verification_stats;
        let verification_mode = match self.state.executor.config.verification {
            deriva_compute::async_executor::VerificationMode::Off => "off".to_string(),
            deriva_compute::async_executor::VerificationMode::DualCompute => "dual".to_string(),
            deriva_compute::async_executor::VerificationMode::Sampled { rate } => {
                format!("sampled:{}", rate)
            }
        };

        Ok(Response::new(StatusResponse {
            recipe_count: self.state.dag.len() as u64,
            blob_count: 0,
            cache_entries: self.state.cache.entry_count().await as u64,
            cache_size_bytes: self.state.cache.current_size().await,
            cache_hit_rate: self.state.cache.hit_rate().await,
            verification_mode,
            verification_total: stats.total_verified.load(std::sync::atomic::Ordering::Relaxed),
            verification_passed: stats.total_passed.load(std::sync::atomic::Ordering::Relaxed),
            verification_failed: stats.total_failed.load(std::sync::atomic::Ordering::Relaxed),
            verification_failure_rate: stats.failure_rate(),
        }))
    }

    async fn verify(
        &self,
        request: Request<VerifyRequest>,
    ) -> Result<Response<VerifyResponse>, Status> {
        use std::time::Instant;
        
        let addr_bytes = request.into_inner().addr;
        let addr = parse_addr(&addr_bytes)?;

        // Get recipe and inputs
        let recipe = self.state.recipes.get(&addr)
            .map_err(|e| Status::internal(format!("recipe lookup failed: {}", e)))?
            .ok_or_else(|| Status::not_found("address not found"))?;

        let func = self.state.registry.get(&recipe.function_id)
            .ok_or_else(|| Status::not_found(format!("function not found: {}", recipe.function_id)))?;

        // Resolve inputs
        let mut input_bytes = Vec::new();
        for input_addr in &recipe.inputs {
            let bytes = self.state.executor.materialize(input_addr.clone()).await
                .map_err(|e| Status::internal(format!("input resolution failed: {}", e)))?;
            input_bytes.push(bytes);
        }

        // Dual compute with timing
        let start = Instant::now();
        let func1 = std::sync::Arc::clone(&func);
        let func2 = std::sync::Arc::clone(&func);
        let input1 = input_bytes.clone();
        let input2 = input_bytes;
        let params1 = recipe.params.clone();
        let params2 = recipe.params.clone();

        let (result1, result2) = tokio::join!(
            tokio::task::spawn_blocking(move || func1.execute(input1, &params1)),
            tokio::task::spawn_blocking(move || func2.execute(input2, &params2))
        );
        let elapsed = start.elapsed();

        let output1 = result1
            .map_err(|e| Status::internal(format!("compute task failed: {}", e)))?
            .map_err(|e| Status::internal(format!("compute failed: {}", e)))?;
        let output2 = result2
            .map_err(|e| Status::internal(format!("compute task failed: {}", e)))?
            .map_err(|e| Status::internal(format!("compute failed: {}", e)))?;

        let deterministic = output1 == output2;
        let hash = blake3::hash(&output1);

        Ok(Response::new(VerifyResponse {
            deterministic,
            output_hash: hash.to_hex().to_string(),
            output_size: output1.len() as u64,
            compute_time_us: elapsed.as_micros() as u64,
            error: if deterministic {
                String::new()
            } else {
                let hash2 = blake3::hash(&output2);
                format!("outputs differ: {} bytes (hash {}) vs {} bytes (hash {})",
                    output1.len(), hash.to_hex(), output2.len(), hash2.to_hex())
            },
        }))
    }
}
