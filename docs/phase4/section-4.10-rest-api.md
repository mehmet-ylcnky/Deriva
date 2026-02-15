# Phase 4, Section 4.10: REST API

**Status:** Blueprint  
**Depends on:** §4.1 (Canonical Serialization), §4.3 (Reproducibility Proofs), §4.5 (Content Integrity), §4.6 (WASM), §4.8 (Chunks), §4.9 (Mutable Refs)

---

## 1. Problem Statement

### 1.1 Current State

Deriva is accessed via **Rust library and gRPC**:

```rust
// Current approach: Rust library
let dag = DagStore::new(...);
let bytes = dag.get(addr).await?;

// Or gRPC (requires protobuf)
let client = DerivaClient::connect("http://localhost:50051").await?;
let response = client.get(GetRequest { addr }).await?;
```

**Problems:**
1. **No HTTP access**: Can't use curl, wget, browsers
2. **No language-agnostic API**: Requires Rust or gRPC client
3. **No web integration**: Can't embed in web apps
4. **No standard tools**: Can't use Postman, httpie
5. **No CDN integration**: Can't cache with standard HTTP caches

### 1.2 The Problem

**Scenario 1: Web Application**
```javascript
// User wants to fetch content from browser
// ❌ Can't use fetch() — no HTTP endpoint
// ❌ Must use gRPC-web (complex setup)
```

**Scenario 2: CI/CD Pipeline**
```bash
# User wants to download artifact
# ❌ Can't use curl
# ❌ Must install Deriva CLI
```

**Scenario 3: CDN Integration**
```
# User wants to cache content at edge
# ❌ No HTTP caching headers
# ❌ Can't use CloudFront, Fastly, etc.
```

### 1.3 Requirements

1. **RESTful API**: Standard HTTP methods (GET, POST, PUT, DELETE)
2. **JSON responses**: Human-readable, language-agnostic
3. **HTTP Range support**: Partial content (§4.8)
4. **Caching headers**: ETag, Cache-Control, Last-Modified
5. **CORS support**: Cross-origin requests
6. **OpenAPI spec**: Auto-generated documentation
7. **Performance**: <10ms latency for small requests

---

## 2. Design

### 2.1 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        REST API Layer                        │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Content    │  │   Compute    │  │  References  │      │
│  │              │  │              │  │              │      │
│  │ • GET /blob  │  │ • POST /exec │  │ • GET /ref   │      │
│  │ • PUT /blob  │  │ • GET /proof │  │ • PUT /ref   │      │
│  │ • Range      │  │ • GET /wasm  │  │ • POST /cas  │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
├─────────────────────────────────────────────────────────────┤
│                      Deriva Core (§2-4)                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   DagStore   │  │   Executor   │  │   RefStore   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 API Endpoints

```
Content Endpoints:
  GET    /api/v1/blob/{addr}              Get blob by CAddr
  GET    /api/v1/blob/{addr}?range=...   Get byte range
  PUT    /api/v1/blob                     Upload blob
  HEAD   /api/v1/blob/{addr}              Get metadata

Compute Endpoints:
  POST   /api/v1/compute                  Execute recipe
  GET    /api/v1/compute/{addr}           Get computation result
  GET    /api/v1/proof/{addr}             Get reproducibility proof

Reference Endpoints:
  GET    /api/v1/ref/{namespace}/{name}   Get reference
  PUT    /api/v1/ref/{namespace}/{name}   Set reference
  POST   /api/v1/ref/{namespace}/{name}/cas  Compare-and-swap
  GET    /api/v1/ref/{namespace}          List references
  GET    /api/v1/ref/{namespace}/{name}/history  Get history

WASM Endpoints:
  PUT    /api/v1/wasm                     Upload WASM module
  POST   /api/v1/wasm/{addr}/execute     Execute WASM function

Health & Metrics:
  GET    /health                          Health check
  GET    /metrics                         Prometheus metrics
  GET    /openapi.json                    OpenAPI specification
```

### 2.3 Core Implementation

```rust
// crates/deriva-server/src/rest.rs

use axum::{
    Router,
    extract::{Path, State, Query},
    http::{StatusCode, HeaderMap, header},
    response::{Response, IntoResponse},
    Json,
    body::Body,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct AppState {
    pub dag: Arc<DagStore>,
    pub executor: Arc<Mutex<Executor>>,
    pub ref_store: Arc<RefStore>,
    pub wasm_loader: Arc<WasmLoader>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Content endpoints
        .route("/api/v1/blob/:addr", axum::routing::get(get_blob))
        .route("/api/v1/blob/:addr", axum::routing::head(head_blob))
        .route("/api/v1/blob", axum::routing::put(put_blob))
        
        // Compute endpoints
        .route("/api/v1/compute", axum::routing::post(compute))
        .route("/api/v1/compute/:addr", axum::routing::get(get_compute_result))
        .route("/api/v1/proof/:addr", axum::routing::get(get_proof))
        
        // Reference endpoints
        .route("/api/v1/ref/:namespace/:name", axum::routing::get(get_ref))
        .route("/api/v1/ref/:namespace/:name", axum::routing::put(put_ref))
        .route("/api/v1/ref/:namespace/:name/cas", axum::routing::post(cas_ref))
        .route("/api/v1/ref/:namespace", axum::routing::get(list_refs))
        .route("/api/v1/ref/:namespace/:name/history", axum::routing::get(ref_history))
        
        // WASM endpoints
        .route("/api/v1/wasm", axum::routing::put(put_wasm))
        .route("/api/v1/wasm/:addr/execute", axum::routing::post(execute_wasm))
        
        // Health & metrics
        .route("/health", axum::routing::get(health))
        .route("/metrics", axum::routing::get(metrics))
        .route("/openapi.json", axum::routing::get(openapi_spec))
        
        .layer(
            tower::ServiceBuilder::new()
                .layer(tower_http::cors::CorsLayer::permissive())
                .layer(tower_http::trace::TraceLayer::new_for_http())
        )
        .with_state(state)
}
```

### 2.4 Content Endpoints

```rust
// GET /api/v1/blob/{addr}
async fn get_blob(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Parse Range header
    let range = headers.get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_range_header);
    
    match range {
        Some((offset, length)) => {
            // Partial content (§4.8)
            let bytes = state.dag.get_range(addr, offset, length).await
                .map_err(|_| StatusCode::NOT_FOUND)?;
            
            let total_size = state.dag.get(addr).await
                .ok_or(StatusCode::NOT_FOUND)?
                .len();
            
            Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, bytes.len())
                .header(header::CONTENT_RANGE, 
                    format!("bytes {}-{}/{}", offset, offset + length - 1, total_size))
                .header(header::ETAG, format!("\"{}\"", addr.to_hex()))
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(Body::from(bytes))
                .unwrap())
        }
        None => {
            // Full content
            let bytes = state.dag.get(addr).await
                .ok_or(StatusCode::NOT_FOUND)?;
            
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, bytes.len())
                .header(header::ETAG, format!("\"{}\"", addr.to_hex()))
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .body(Body::from(bytes))
                .unwrap())
        }
    }
}

// HEAD /api/v1/blob/{addr}
async fn head_blob(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let bytes = state.dag.get(addr).await
        .ok_or(StatusCode::NOT_FOUND)?;
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::ETAG, format!("\"{}\"", addr.to_hex()))
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::empty())
        .unwrap())
}

// PUT /api/v1/blob
#[derive(Deserialize)]
struct PutBlobRequest {
    #[serde(with = "base64")]
    data: Vec<u8>,
}

#[derive(Serialize)]
struct PutBlobResponse {
    addr: String,
    size: usize,
}

async fn put_blob(
    State(state): State<AppState>,
    Json(req): Json<PutBlobRequest>,
) -> Result<Json<PutBlobResponse>, StatusCode> {
    let bytes = Bytes::from(req.data);
    let addr = CAddr::from_bytes(&bytes);
    
    state.dag.put(addr, bytes.clone()).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(PutBlobResponse {
        addr: addr.to_hex(),
        size: bytes.len(),
    }))
}

fn parse_range_header(range: &str) -> Option<(u64, u64)> {
    let range = range.strip_prefix("bytes=")?;
    let parts: Vec<_> = range.split('-').collect();
    
    if parts.len() != 2 {
        return None;
    }
    
    let start: u64 = parts[0].parse().ok()?;
    let end: u64 = parts[1].parse().ok()?;
    
    Some((start, end - start + 1))
}
```

### 2.5 Compute Endpoints

```rust
// POST /api/v1/compute
#[derive(Deserialize)]
struct ComputeRequest {
    function_id: String,
    inputs: Vec<String>,  // CAddr hex strings
    params: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct ComputeResponse {
    addr: String,
    result: String,  // Base64
}

async fn compute(
    State(state): State<AppState>,
    Json(req): Json<ComputeRequest>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    // Parse inputs
    let input_addrs: Vec<CAddr> = req.inputs.iter()
        .map(|s| CAddr::from_hex(s))
        .collect::<Result<_, _>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Create recipe
    let recipe = Recipe {
        function_id: FunctionId::from(req.function_id),
        inputs: input_addrs,
        params: req.params.into_iter()
            .map(|(k, v)| (k, json_to_value(v)))
            .collect(),
    };
    
    let recipe_addr = recipe.addr();
    
    // Execute
    let mut executor = state.executor.lock().await;
    let result = executor.materialize(recipe_addr).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(ComputeResponse {
        addr: recipe_addr.to_hex(),
        result: base64::encode(&result),
    }))
}

// GET /api/v1/compute/{addr}
async fn get_compute_result(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let mut executor = state.executor.lock().await;
    let result = executor.materialize(addr).await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    Ok(Json(ComputeResponse {
        addr: addr.to_hex(),
        result: base64::encode(&result),
    }))
}

// GET /api/v1/proof/{addr}
#[derive(Serialize)]
struct ProofResponse {
    addr: String,
    proof: DerivationProof,
}

async fn get_proof(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
) -> Result<Json<ProofResponse>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Generate proof (§4.3)
    let proof = DerivationProof::generate(&state.dag, addr).await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    Ok(Json(ProofResponse {
        addr: addr.to_hex(),
        proof,
    }))
}
```

### 2.6 Reference Endpoints

```rust
// GET /api/v1/ref/{namespace}/{name}
#[derive(Serialize)]
struct RefResponse {
    addr: String,
    version: u64,
    updated_at: String,
}

async fn get_ref(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<Json<RefResponse>, StatusCode> {
    let ref_name = RefName::new(namespace, name);
    
    let value = state.ref_store.get(&ref_name)
        .ok_or(StatusCode::NOT_FOUND)?;
    
    Ok(Json(RefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

// PUT /api/v1/ref/{namespace}/{name}
#[derive(Deserialize)]
struct PutRefRequest {
    addr: String,
}

async fn put_ref(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Json(req): Json<PutRefRequest>,
) -> Result<Json<RefResponse>, StatusCode> {
    let ref_name = RefName::new(namespace, name);
    let addr = CAddr::from_hex(&req.addr)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let value = state.ref_store.set(ref_name, addr);
    
    Ok(Json(RefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

// POST /api/v1/ref/{namespace}/{name}/cas
#[derive(Deserialize)]
struct CasRefRequest {
    expected_version: u64,
    new_addr: String,
}

async fn cas_ref(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Json(req): Json<CasRefRequest>,
) -> Result<Json<RefResponse>, StatusCode> {
    let ref_name = RefName::new(namespace, name);
    let new_addr = CAddr::from_hex(&req.new_addr)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let value = state.ref_store.cas(ref_name, req.expected_version, new_addr)
        .map_err(|_| StatusCode::CONFLICT)?;
    
    Ok(Json(RefResponse {
        addr: value.addr.to_hex(),
        version: value.version,
        updated_at: format!("{:?}", value.updated_at),
    }))
}

// GET /api/v1/ref/{namespace}
#[derive(Serialize)]
struct ListRefsResponse {
    refs: Vec<RefEntry>,
}

#[derive(Serialize)]
struct RefEntry {
    name: String,
    addr: String,
    version: u64,
}

async fn list_refs(
    State(state): State<AppState>,
    Path(namespace): Path<String>,
) -> Json<ListRefsResponse> {
    let refs = state.ref_store.list(&namespace);
    
    let entries = refs.into_iter()
        .map(|(name, value)| RefEntry {
            name: name.name,
            addr: value.addr.to_hex(),
            version: value.version,
        })
        .collect();
    
    Json(ListRefsResponse { refs: entries })
}

// GET /api/v1/ref/{namespace}/{name}/history
#[derive(Serialize)]
struct HistoryResponse {
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize)]
struct HistoryEntry {
    version: u64,
    addr: String,
    updated_at: String,
}

async fn ref_history(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
) -> Json<HistoryResponse> {
    let ref_name = RefName::new(namespace, name);
    let history = state.ref_store.history(&ref_name);
    
    let entries = history.into_iter()
        .map(|entry| HistoryEntry {
            version: entry.version,
            addr: entry.addr.to_hex(),
            updated_at: format!("{:?}", entry.updated_at),
        })
        .collect();
    
    Json(HistoryResponse { entries })
}
```

### 2.7 WASM Endpoints

```rust
// PUT /api/v1/wasm
#[derive(Deserialize)]
struct PutWasmRequest {
    #[serde(with = "base64")]
    wasm_bytes: Vec<u8>,
}

#[derive(Serialize)]
struct PutWasmResponse {
    addr: String,
}

async fn put_wasm(
    State(state): State<AppState>,
    Json(req): Json<PutWasmRequest>,
) -> Result<Json<PutWasmResponse>, StatusCode> {
    let wasm_bytes = Bytes::from(req.wasm_bytes);
    
    // Validate WASM
    state.wasm_loader.validate(&wasm_bytes)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Store
    let addr = CAddr::from_bytes(&wasm_bytes);
    state.dag.put(addr, wasm_bytes).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(PutWasmResponse {
        addr: addr.to_hex(),
    }))
}

// POST /api/v1/wasm/{addr}/execute
#[derive(Deserialize)]
struct ExecuteWasmRequest {
    inputs: Vec<String>,  // Base64
    params: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct ExecuteWasmResponse {
    result: String,  // Base64
}

async fn execute_wasm(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
    Json(req): Json<ExecuteWasmRequest>,
) -> Result<Json<ExecuteWasmResponse>, StatusCode> {
    let addr = CAddr::from_hex(&addr_hex)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Load WASM function
    let wasm_func = state.wasm_loader.load(&state.dag, addr).await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    
    // Parse inputs
    let inputs: Vec<Bytes> = req.inputs.iter()
        .map(|s| base64::decode(s).map(Bytes::from))
        .collect::<Result<_, _>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    // Execute
    let result = wasm_func.execute(inputs, &req.params.into_iter()
        .map(|(k, v)| (k, json_to_value(v)))
        .collect())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(ExecuteWasmResponse {
        result: base64::encode(&result),
    }))
}
```

### 2.8 Health & Metrics

```rust
// GET /health
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// GET /metrics
async fn metrics() -> String {
    // Prometheus metrics
    prometheus::TextEncoder::new()
        .encode_to_string(&prometheus::gather())
        .unwrap()
}

// GET /openapi.json
async fn openapi_spec() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Deriva API",
            "version": "1.0.0",
            "description": "Content-addressed distributed file system"
        },
        "paths": {
            "/api/v1/blob/{addr}": {
                "get": {
                    "summary": "Get blob by CAddr",
                    "parameters": [{
                        "name": "addr",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    }],
                    "responses": {
                        "200": { "description": "Blob content" },
                        "404": { "description": "Not found" }
                    }
                }
            }
            // ... more endpoints ...
        }
    }))
}
```

---

## 3. Implementation

### 3.1 Server Startup

```rust
// crates/deriva-server/src/main.rs

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize
    let dag = Arc::new(DagStore::new_sled("./data")?);
    let ref_store = Arc::new(RefStore::new());
    let registry = Arc::new(FunctionRegistry::new());
    let cache = Arc::new(Mutex::new(ExecutionCache::new(1000)));
    let leaf_store = Arc::new(LeafStore::new_memory());
    let wasm_loader = Arc::new(WasmLoader::new(WasmConfig::default()));
    
    let executor = Arc::new(Mutex::new(Executor::new(
        &dag,
        &registry,
        &mut cache.lock().await,
        &leaf_store,
        ExecutorConfig::default(),
    )));
    
    let state = AppState {
        dag,
        executor,
        ref_store,
        wasm_loader,
    };
    
    // Create router
    let app = create_router(state);
    
    // Start server
    let addr = "0.0.0.0:8080".parse()?;
    println!("Listening on {}", addr);
    
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}
```

### 3.2 Client Library

```rust
// crates/deriva-client/src/lib.rs

pub struct DerivaClient {
    base_url: String,
    client: reqwest::Client,
}

impl DerivaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
    
    pub async fn get_blob(&self, addr: CAddr) -> Result<Bytes, ClientError> {
        let url = format!("{}/api/v1/blob/{}", self.base_url, addr.to_hex());
        
        let response = self.client.get(&url)
            .send()
            .await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ClientError::NotFound);
        }
        
        let bytes = response.bytes().await?;
        Ok(bytes)
    }
    
    pub async fn put_blob(&self, data: Bytes) -> Result<CAddr, ClientError> {
        let url = format!("{}/api/v1/blob", self.base_url);
        
        let response = self.client.put(&url)
            .json(&serde_json::json!({
                "data": base64::encode(&data)
            }))
            .send()
            .await?;
        
        let result: PutBlobResponse = response.json().await?;
        Ok(CAddr::from_hex(&result.addr)?)
    }
    
    pub async fn compute(
        &self,
        function_id: FunctionId,
        inputs: Vec<CAddr>,
        params: BTreeMap<String, Value>,
    ) -> Result<Bytes, ClientError> {
        let url = format!("{}/api/v1/compute", self.base_url);
        
        let response = self.client.post(&url)
            .json(&serde_json::json!({
                "function_id": function_id.as_str(),
                "inputs": inputs.iter().map(|a| a.to_hex()).collect::<Vec<_>>(),
                "params": params
            }))
            .send()
            .await?;
        
        let result: ComputeResponse = response.json().await?;
        Ok(Bytes::from(base64::decode(&result.result)?))
    }
    
    pub async fn get_ref(&self, name: &RefName) -> Result<RefValue, ClientError> {
        let url = format!("{}/api/v1/ref/{}/{}", 
            self.base_url, name.namespace, name.name);
        
        let response = self.client.get(&url)
            .send()
            .await?;
        
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ClientError::NotFound);
        }
        
        let result: RefResponse = response.json().await?;
        Ok(RefValue {
            addr: CAddr::from_hex(&result.addr)?,
            version: result.version,
            updated_at: SystemTime::now(),  // Approximate
            metadata: BTreeMap::new(),
        })
    }
    
    pub async fn set_ref(&self, name: RefName, addr: CAddr) -> Result<RefValue, ClientError> {
        let url = format!("{}/api/v1/ref/{}/{}", 
            self.base_url, name.namespace, name.name);
        
        let response = self.client.put(&url)
            .json(&serde_json::json!({
                "addr": addr.to_hex()
            }))
            .send()
            .await?;
        
        let result: RefResponse = response.json().await?;
        Ok(RefValue {
            addr: CAddr::from_hex(&result.addr)?,
            version: result.version,
            updated_at: SystemTime::now(),
            metadata: BTreeMap::new(),
        })
    }
}
```

---

## 4. Data Flow Diagrams

### 4.1 GET Blob Flow

```
┌──────┐                                                      ┌────────┐
│Client│                                                      │ Server │
└──┬───┘                                                      └───┬────┘
   │                                                               │
   │ GET /api/v1/blob/abc123...                                   │
   │──────────────────────────────────────────────────────────────>
   │                                                               │
   │                                          Parse CAddr          │
   │                                          ─────────────        │
   │                                                               │
   │                                          dag.get(addr)        │
   │                                          ─────────────        │
   │                                                               │
   │                                          Found                │
   │                                          ─────────────        │
   │                                                               │
   │ 200 OK                                                       │
   │ ETag: "abc123..."                                            │
   │ Cache-Control: immutable                                     │
   │ <blob bytes>                                                 │
   │<──────────────────────────────────────────────────────────────
   │                                                               │
```

### 4.2 POST Compute Flow

```
┌──────┐                                                      ┌────────┐
│Client│                                                      │ Server │
└──┬───┘                                                      └───┬────┘
   │                                                               │
   │ POST /api/v1/compute                                         │
   │ { function_id, inputs, params }                              │
   │──────────────────────────────────────────────────────────────>
   │                                                               │
   │                                          Create recipe        │
   │                                          ─────────────        │
   │                                                               │
   │                                          executor.materialize │
   │                                          ─────────────        │
   │                                                               │
   │                                          Compute result       │
   │                                          ─────────────        │
   │                                                               │
   │ 200 OK                                                       │
   │ { addr, result }                                             │
   │<──────────────────────────────────────────────────────────────
   │                                                               │
```

### 4.3 PUT Reference Flow

```
┌──────┐                                                      ┌────────┐
│Client│                                                      │ Server │
└──┬───┘                                                      └───┬────┘
   │                                                               │
   │ PUT /api/v1/ref/user:alice/config                            │
   │ { addr: "def456..." }                                        │
   │──────────────────────────────────────────────────────────────>
   │                                                               │
   │                                          ref_store.set()      │
   │                                          ─────────────        │
   │                                                               │
   │                                          Version incremented  │
   │                                          ─────────────        │
   │                                                               │
   │ 200 OK                                                       │
   │ { addr, version: 2 }                                         │
   │<──────────────────────────────────────────────────────────────
   │                                                               │
```

---

## 5. Test Specification

### 5.1 Integration Tests

```rust
// crates/deriva-server/tests/rest_api.rs

#[tokio::test]
async fn test_get_blob() {
    let app = create_test_app().await;
    
    // Put blob
    let data = Bytes::from("hello world");
    let addr = CAddr::from_bytes(&data);
    app.state.dag.put(addr, data.clone()).await.unwrap();
    
    // Get blob
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/blob/{}", addr.to_hex()))
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, data);
}

#[tokio::test]
async fn test_get_blob_not_found() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/blob/nonexistent")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_put_blob() {
    let app = create_test_app().await;
    
    let data = Bytes::from("test data");
    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/blob")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&serde_json::json!({
                    "data": base64::encode(&data)
                })).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body: PutBlobResponse = serde_json::from_slice(
        &hyper::body::to_bytes(response.into_body()).await.unwrap()
    ).unwrap();
    
    let addr = CAddr::from_hex(&body.addr).unwrap();
    assert_eq!(addr, CAddr::from_bytes(&data));
}

#[tokio::test]
async fn test_range_request() {
    let app = create_test_app().await;
    
    let data = Bytes::from(vec![0u8; 1000]);
    let addr = CAddr::from_bytes(&data);
    app.state.dag.put(addr, data.clone()).await.unwrap();
    
    // Request bytes 100-199
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/blob/{}", addr.to_hex()))
                .header("Range", "bytes=100-199")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body.len(), 100);
}
```

---

## 6. Edge Cases & Error Handling

### 6.1 Invalid CAddr Format

```rust
#[tokio::test]
async fn test_invalid_addr() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/blob/invalid-hex")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

### 6.2 CAS Conflict

```rust
#[tokio::test]
async fn test_cas_conflict() {
    let app = create_test_app().await;
    
    // Set initial value
    let ref_name = RefName::new("test", "config");
    let addr1 = CAddr::from_hex("aaa111...").unwrap();
    app.state.ref_store.set(ref_name.clone(), addr1);
    
    // Try CAS with wrong version
    let addr2 = CAddr::from_hex("bbb222...").unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/ref/test/config/cas")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&serde_json::json!({
                    "expected_version": 999,
                    "new_addr": addr2.to_hex()
                })).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
```

---

## 7. Performance Analysis

### 7.1 Latency

**GET /blob (1 KB):** ~5 ms
- Parse: 0.1 ms
- DagStore lookup: 1 ms
- Serialization: 0.1 ms
- Network: 3 ms

**POST /compute:** ~50 ms
- Parse: 1 ms
- Execution: 45 ms
- Serialization: 1 ms
- Network: 3 ms

**PUT /ref:** ~2 ms
- Parse: 0.1 ms
- RefStore update: 1 ms
- Serialization: 0.1 ms
- Network: 0.8 ms

### 7.2 Throughput

**GET /blob:** ~10,000 req/sec (single core)
**POST /compute:** ~20 req/sec (CPU-bound)
**PUT /ref:** ~5,000 req/sec (lock contention)

---

## 8. Files Changed

### New Files
- `crates/deriva-server/src/rest.rs` — REST API implementation
- `crates/deriva-server/src/main.rs` — Server startup
- `crates/deriva-client/src/lib.rs` — HTTP client library
- `crates/deriva-server/tests/rest_api.rs` — Integration tests

### Modified Files
- `Cargo.toml` — Add deriva-server and deriva-client crates

---

## 9. Dependency Changes

```toml
# crates/deriva-server/Cargo.toml
[dependencies]
axum = "0.7"
tokio = { version = "1.35", features = ["full"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
base64 = "0.21"
prometheus = "0.13"

# crates/deriva-client/Cargo.toml
[dependencies]
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1.35", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
base64 = "0.21"
```

---

## 10. Design Rationale

### 10.1 Why Axum?

**Alternatives:** actix-web, warp, rocket

**Decision:** Axum is built on tokio, has excellent ergonomics, and strong type safety.

### 10.2 Why Immutable Cache Headers?

**Reason:** Content-addressed data never changes — perfect for aggressive caching.

### 10.3 Why Base64 for Binary Data?

**Alternative:** Multipart form data.

**Decision:** Base64 in JSON is simpler for API clients, though less efficient.

### 10.4 Why Separate Client Library?

**Reason:** Provides idiomatic Rust API, hides HTTP details.

---

## 11. Observability Integration

### 11.1 Metrics

```rust
lazy_static! {
    static ref HTTP_REQUESTS: IntCounterVec = register_int_counter_vec!(
        "deriva_http_requests_total",
        "HTTP requests by method, path, and status",
        &["method", "path", "status"]
    ).unwrap();

    static ref HTTP_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_http_request_duration_seconds",
        "HTTP request duration",
        &["method", "path"]
    ).unwrap();
}
```

### 11.2 Tracing

```rust
use tracing::{info_span, instrument};

#[instrument(skip(state))]
async fn get_blob(
    State(state): State<AppState>,
    Path(addr_hex): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let span = info_span!("get_blob", addr = %addr_hex);
    let _enter = span.enter();
    
    // ... implementation ...
}
```

---

## 12. Checklist

### Implementation
- [ ] Create `deriva-server/src/rest.rs` with REST API
- [ ] Implement content endpoints (GET, PUT, HEAD)
- [ ] Implement compute endpoints (POST, GET proof)
- [ ] Implement reference endpoints (GET, PUT, CAS, list, history)
- [ ] Implement WASM endpoints (PUT, execute)
- [ ] Add health and metrics endpoints
- [ ] Add OpenAPI spec endpoint
- [ ] Create `deriva-client/src/lib.rs` with HTTP client
- [ ] Add CORS support
- [ ] Add HTTP Range support
- [ ] Add caching headers (ETag, Cache-Control)

### Testing
- [ ] Integration test: GET blob
- [ ] Integration test: GET blob not found
- [ ] Integration test: PUT blob
- [ ] Integration test: Range request
- [ ] Integration test: POST compute
- [ ] Integration test: GET/PUT/CAS reference
- [ ] Integration test: PUT/execute WASM
- [ ] Integration test: invalid CAddr
- [ ] Integration test: CAS conflict
- [ ] Load test: 10K req/sec for GET
- [ ] Load test: concurrent requests

### Documentation
- [ ] Document all API endpoints
- [ ] Add curl examples
- [ ] Document error codes
- [ ] Add authentication guide (future)
- [ ] Document rate limiting (future)
- [ ] Add OpenAPI spec

### Observability
- [ ] Add `deriva_http_requests_total` counter
- [ ] Add `deriva_http_request_duration_seconds` histogram
- [ ] Add tracing spans for all endpoints
- [ ] Add access logs

### Validation
- [ ] Test with curl
- [ ] Test with Postman
- [ ] Test with browser
- [ ] Verify caching headers work
- [ ] Verify Range requests work
- [ ] Load test (target: 10K req/sec)

### Deployment
- [ ] Deploy REST API server
- [ ] Monitor HTTP metrics
- [ ] Set up reverse proxy (nginx)
- [ ] Configure TLS
- [ ] Add rate limiting
- [ ] Document API endpoints

---

**Estimated effort:** 5–7 days
- Days 1-2: Core REST endpoints (content, compute, refs)
- Day 3: WASM endpoints + client library
- Day 4: Health, metrics, OpenAPI spec
- Days 5-6: Tests + load testing
- Day 7: Documentation + deployment

**Success criteria:**
1. All endpoints work correctly
2. HTTP Range requests work
3. Caching headers are correct
4. GET throughput >10K req/sec
5. Client library works
6. OpenAPI spec is complete
7. Load tests pass
