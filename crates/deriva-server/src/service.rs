use crate::metrics::*;
use crate::state::ServerState;
use deriva_core::address::*;
use deriva_core::invalidation::CascadePolicy;
use deriva_core::streaming::StreamChunk;
use deriva_compute::async_executor::CombinedDagReader;
use deriva_compute::cache::AsyncMaterializationCache;
use deriva_compute::invalidation::CascadeInvalidator;
use deriva_compute::pipeline::PipelineConfig;
use deriva_compute::streaming::{collect_stream, tee_stream};
use deriva_compute::streaming_executor::StreamingExecutor;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use uuid::Uuid;

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

fn parse_cascade_policy(s: &str) -> CascadePolicy {
    match s.to_lowercase().as_str() {
        "none" => CascadePolicy::None,
        "dry_run" | "dryrun" => CascadePolicy::DryRun,
        _ => CascadePolicy::Immediate,
    }
}

fn record_rpc(method: &str, start: Instant, ok: bool) {
    let status = if ok { "ok" } else { "error" };
    RPC_TOTAL.with_label_values(&[method, status]).inc();
    RPC_DURATION.with_label_values(&[method]).observe(start.elapsed().as_secs_f64());
    RPC_ACTIVE.with_label_values(&[method]).dec();
}

fn begin_rpc(method: &str) -> (Instant, tracing::Span) {
    RPC_ACTIVE.with_label_values(&[method]).inc();
    let request_id = Uuid::new_v4().to_string();
    let span = tracing::info_span!("rpc", method = method, request_id = %request_id);
    (Instant::now(), span)
}

#[tonic::async_trait]
impl Deriva for DerivaService {
    async fn put_leaf(
        &self,
        request: Request<PutLeafRequest>,
    ) -> Result<Response<PutLeafResponse>, Status> {
        let (start, span) = begin_rpc("put_leaf");
        let _enter = span.enter();
        let result = self
            .state
            .storage
            .put_leaf(&request.get_ref().data)
            .map_err(|e| Status::internal(e.to_string()));
        let ok = result.is_ok();
        let resp = result.map(|addr| {
            DAG_RECIPES.set(self.state.dag.len() as f64);
            Response::new(PutLeafResponse { addr: addr.as_bytes().to_vec() })
        });
        record_rpc("put_leaf", start, ok);
        resp
    }

    async fn put_recipe(
        &self,
        request: Request<PutRecipeRequest>,
    ) -> Result<Response<PutRecipeResponse>, Status> {
        let (start, span) = begin_rpc("put_recipe");
        let _enter = span.enter();
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
        let result = self
            .state
            .storage
            .put_recipe(&recipe)
            .map_err(|e| Status::internal(e.to_string()));
        let ok = result.is_ok();
        let resp = result.map(|addr| {
            DAG_INSERT_DURATION.observe(start.elapsed().as_secs_f64());
            DAG_RECIPES.set(self.state.dag.len() as f64);
            Response::new(PutRecipeResponse { addr: addr.as_bytes().to_vec() })
        });
        record_rpc("put_recipe", start, ok);
        resp
    }

    type GetStream = ReceiverStream<Result<GetResponse, Status>>;

    async fn get(
        &self,
        request: Request<GetRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let (start, span) = begin_rpc("get");
        let addr = parse_addr(&request.get_ref().addr)?;
        let state = Arc::clone(&self.state);

        // Check if root function supports streaming
        let use_streaming = state.recipes.get(&addr)
            .ok()
            .flatten()
            .map(|r| state.registry.has_streaming(&r.function_id))
            .unwrap_or(false);

        if use_streaming {
            let streaming_executor = StreamingExecutor::new(PipelineConfig::default());
            let dag_reader = CombinedDagReader {
                dag: Arc::clone(&state.dag),
                recipes: Arc::clone(&state.recipes),
            };
            let compute_rx = streaming_executor.materialize_streaming(
                &addr, &dag_reader,
                state.cache.as_ref(), state.blobs.as_ref(), &state.registry,
            ).await.map_err(|e| Status::internal(e.to_string()))?;

            // Tee: client stream + background cache collector
            let (client_rx, cache_rx) = tee_stream(compute_rx, 8);
            let cache = Arc::clone(&state.cache);
            let cache_addr = addr;

            // Convert client_rx to gRPC stream
            let (grpc_tx, grpc_rx) = mpsc::channel(8);
            tokio::spawn(async move {
                let mut client_rx = client_rx;
                loop {
                    match client_rx.recv().await {
                        Some(StreamChunk::Data(chunk)) => {
                            if grpc_tx.send(Ok(GetResponse { chunk: chunk.to_vec() })).await.is_err() {
                                break;
                            }
                        }
                        Some(StreamChunk::End) | None => break,
                        Some(StreamChunk::Error(e)) => {
                            let _ = grpc_tx.send(Err(Status::internal(e.to_string()))).await;
                            break;
                        }
                    }
                }
                record_rpc("get", start, true);
            }.instrument(span));

            tokio::spawn(async move {
                if let Ok(full_data) = collect_stream(cache_rx).await {
                    cache.put(cache_addr, full_data).await;
                }
            });

            Ok(Response::new(ReceiverStream::new(grpc_rx)))
        } else {
            // Batch path (existing behavior)
            let (tx, rx) = mpsc::channel(16);
            tokio::spawn(async move {
                let result = state.executor.materialize(addr).await;
                match result {
                    Ok(data) => {
                        for chunk in data.chunks(CHUNK_SIZE) {
                            if tx.send(Ok(GetResponse { chunk: chunk.to_vec() })).await.is_err() {
                                break;
                            }
                        }
                        record_rpc("get", start, true);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                        record_rpc("get", start, false);
                    }
                }
            }.instrument(span));

            Ok(Response::new(ReceiverStream::new(rx)))
        }
    }

    async fn resolve(
        &self,
        request: Request<ResolveRequest>,
    ) -> Result<Response<ResolveResponse>, Status> {
        let (start, span) = begin_rpc("resolve");
        let _enter = span.enter();
        let addr = parse_addr(&request.get_ref().addr)?;
        let result = match self.state.recipes.get(&addr)
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
        };
        record_rpc("resolve", start, result.is_ok());
        result
    }

    async fn invalidate(
        &self,
        request: Request<InvalidateRequest>,
    ) -> Result<Response<InvalidateResponse>, Status> {
        let (start, span) = begin_rpc("invalidate");
        let _enter = span.enter();
        let req = request.get_ref();
        let addr = parse_addr(&req.addr)?;

        let result = if req.cascade {
            let result = CascadeInvalidator::invalidate(
                &self.state.dag, &self.state.cache, &addr,
                CascadePolicy::Immediate, true, false,
            ).await;
            CASCADE_TOTAL.with_label_values(&["immediate"]).inc();
            CASCADE_EVICTED.observe(result.evicted_count as f64);
            CASCADE_DEPTH.observe(result.max_depth as f64);
            CASCADE_DURATION.observe(result.duration.as_secs_f64());
            Ok(Response::new(InvalidateResponse {
                was_cached: result.evicted_count > 0,
                evicted_count: result.evicted_count,
            }))
        } else {
            let was_cached = self.state.cache.remove(&addr).await.is_some();
            Ok(Response::new(InvalidateResponse {
                was_cached,
                evicted_count: if was_cached { 1 } else { 0 },
            }))
        };
        record_rpc("invalidate", start, result.is_ok());
        result
    }

    async fn cascade_invalidate(
        &self,
        request: Request<CascadeInvalidateRequest>,
    ) -> Result<Response<CascadeInvalidateResponse>, Status> {
        let (start, span) = begin_rpc("cascade_invalidate");
        let _enter = span.enter();
        let req = request.get_ref();
        let addr = parse_addr(&req.addr)?;
        let policy = parse_cascade_policy(&req.policy);

        let policy_label = match policy {
            CascadePolicy::None => "none",
            CascadePolicy::Immediate => "immediate",
            CascadePolicy::DryRun => "dry_run",
        };

        let result = CascadeInvalidator::invalidate(
            &self.state.dag, &self.state.cache, &addr,
            policy, req.include_root, req.detail_addrs,
        ).await;

        CASCADE_TOTAL.with_label_values(&[policy_label]).inc();
        CASCADE_EVICTED.observe(result.evicted_count as f64);
        CASCADE_DEPTH.observe(result.max_depth as f64);
        CASCADE_DURATION.observe(result.duration.as_secs_f64());

        let resp = Ok(Response::new(CascadeInvalidateResponse {
            evicted_count: result.evicted_count,
            traversed_count: result.traversed_count,
            max_depth: result.max_depth,
            bytes_reclaimed: result.bytes_reclaimed,
            evicted_addrs: result.evicted_addrs.iter().map(|a| a.as_bytes().to_vec()).collect(),
            duration_micros: result.duration.as_micros() as u64,
        }));
        record_rpc("cascade_invalidate", start, true);
        resp
    }

    async fn status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let (start, span) = begin_rpc("status");
        let _enter = span.enter();
        let stats = &self.state.executor.verification_stats;
        let verification_mode = match self.state.executor.config.verification {
            deriva_compute::async_executor::VerificationMode::Off => "off".to_string(),
            deriva_compute::async_executor::VerificationMode::DualCompute => "dual".to_string(),
            deriva_compute::async_executor::VerificationMode::Sampled { rate } => {
                format!("sampled:{}", rate)
            }
        };

        let cache_entries = self.state.cache.entry_count().await as u64;
        let cache_size = self.state.cache.current_size().await;
        CACHE_ENTRIES.set(cache_entries as f64);
        CACHE_SIZE.set(cache_size as f64);
        CACHE_HIT_RATE.set(self.state.cache.hit_rate().await);
        DAG_RECIPES.set(self.state.dag.len() as f64);
        VERIFY_FAILURE_RATE.set(stats.failure_rate());

        let resp = Ok(Response::new(StatusResponse {
            recipe_count: self.state.dag.len() as u64,
            blob_count: 0,
            cache_entries,
            cache_size_bytes: cache_size,
            cache_hit_rate: self.state.cache.hit_rate().await,
            verification_mode,
            verification_total: stats.total_verified.load(std::sync::atomic::Ordering::Relaxed),
            verification_passed: stats.total_passed.load(std::sync::atomic::Ordering::Relaxed),
            verification_failed: stats.total_failed.load(std::sync::atomic::Ordering::Relaxed),
            verification_failure_rate: stats.failure_rate(),
        }));
        record_rpc("status", start, true);
        resp
    }

    async fn verify(
        &self,
        request: Request<VerifyRequest>,
    ) -> Result<Response<VerifyResponse>, Status> {
        let (start, span) = begin_rpc("verify");
        let _enter = span.enter();
        let addr_bytes = request.into_inner().addr;
        let addr = parse_addr(&addr_bytes)?;

        let recipe = self.state.recipes.get(&addr)
            .map_err(|e| Status::internal(format!("recipe lookup failed: {}", e)))?
            .ok_or_else(|| Status::not_found("address not found"))?;

        let func = self.state.registry.get(&recipe.function_id)
            .ok_or_else(|| Status::not_found(format!("function not found: {}", recipe.function_id)))?;

        let mut input_bytes = Vec::new();
        for input_addr in &recipe.inputs {
            let bytes = self.state.executor.materialize(*input_addr).await
                .map_err(|e| Status::internal(format!("input resolution failed: {}", e)))?;
            input_bytes.push(bytes);
        }

        let compute_start = Instant::now();
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
        let elapsed = compute_start.elapsed();

        let output1 = result1
            .map_err(|e| Status::internal(format!("compute task failed: {}", e)))?
            .map_err(|e| Status::internal(format!("compute failed: {}", e)))?;
        let output2 = result2
            .map_err(|e| Status::internal(format!("compute task failed: {}", e)))?
            .map_err(|e| Status::internal(format!("compute failed: {}", e)))?;

        let deterministic = output1 == output2;
        let hash = blake3::hash(&output1);

        let result_label = if deterministic { "pass" } else { "fail" };
        VERIFY_TOTAL.with_label_values(&[result_label]).inc();

        let resp = Ok(Response::new(VerifyResponse {
            deterministic,
            output_hash: hash.to_hex().to_string(),
            output_size: output1.len() as u64,
            compute_time_us: elapsed.as_micros() as u64,
            error: if deterministic {
                String::new()
            } else {
                "non-deterministic output".to_string()
            },
        }));
        record_rpc("verify", start, true);
        resp
    }
}
