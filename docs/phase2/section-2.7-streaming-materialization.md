# §2.7 Streaming Materialization

> **Status**: Blueprint  
> **Depends on**: §2.2 Async Compute Engine, §2.3 Parallel Materialization  
> **Crate(s)**: `deriva-compute`, `deriva-core`, `deriva-server`  
> **Estimated effort**: 3–4 days  

---

## 1. Problem Statement

### 1.1 Current Limitation

The existing `ComputeFunction` trait requires all inputs to be fully
materialized in memory before computation begins:

```rust
// Current trait (from crates/deriva-compute/src/lib.rs)
pub trait ComputeFunction: Send + Sync {
    fn execute(
        &self,
        inputs: &[Bytes],
        params: &HashMap<String, String>,
    ) -> Result<Bytes, DerivaError>;
}
```

This means:
1. **All inputs must fit in memory simultaneously** — a function with 10
   inputs of 100MB each requires 1GB of RAM just for the input buffers
2. **No incremental output** — the function must produce the entire result
   before any byte is returned to the caller
3. **No pipeline parallelism** — upstream computation must fully complete
   before downstream can start
4. **Wasted memory** — inputs are held in memory even after the function
   has consumed them

### 1.2 Real-World Impact

```
Example: Video transcoding pipeline

  Raw frames (2GB) ──▶ Encode H.264 ──▶ Package MP4 ──▶ Encrypt DRM

  Current behavior:
    1. Materialize raw frames: 2GB in memory
    2. Encode: reads 2GB, produces 200MB → peak 2.2GB
    3. Package: reads 200MB, produces 210MB → peak 410MB
    4. Encrypt: reads 210MB, produces 210MB → peak 420MB
    Total peak memory: 2.2GB
    Total wall time: T_encode + T_package + T_encrypt (sequential)

  With streaming:
    1. Raw frames streamed in 64KB chunks
    2. Encode processes each chunk, emits compressed chunks
    3. Package receives compressed chunks as they arrive
    4. Encrypt receives packaged chunks as they arrive
    Total peak memory: ~256KB (4 × 64KB buffers)
    Total wall time: max(T_encode, T_package, T_encrypt) (pipelined)
```

### 1.3 Comparison

| Aspect | Batch (current) | Streaming (§2.7) |
|--------|-----------------|-------------------|
| Memory | O(total input size) | O(chunk_size × pipeline_depth) |
| Latency to first byte | After full computation | After first chunk processed |
| Pipeline parallelism | None | Full overlap |
| Backpressure | N/A | Bounded channel capacity |
| Complexity | Simple | Moderate |
| Suitable for | Small values (<1MB) | Any size, especially >10MB |
| Cancellation | All-or-nothing | Graceful mid-stream |

### 1.4 What Already Exists

The gRPC `get` RPC already streams output to the client:

```rust
// From service.rs — current streaming output
async fn get(&self, request: Request<GetRequest>)
    -> Result<Response<Self::GetStream>, Status>
{
    // ... materialize full value ...
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        for chunk in data.chunks(65536) {
            tx.send(Ok(GetResponse { chunk: chunk.to_vec() })).await;
        }
    });
    Ok(Response::new(ReceiverStream::new(rx)))
}
```

But the materialization itself is batch — the full `Bytes` value is computed
before chunking begins. §2.7 pushes streaming into the compute layer itself.

---

## 2. Design

### 2.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Streaming Pipeline                     │
│                                                           │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐            │
│  │ Source    │───▶│ Stage 1  │───▶│ Stage 2  │───▶ ...    │
│  │ (chunks) │    │ (compute)│    │ (compute)│            │
│  └──────────┘    └──────────┘    └──────────┘            │
│       │               │               │                   │
│       ▼               ▼               ▼                   │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐            │
│  │ Bounded  │    │ Bounded  │    │ Bounded  │            │
│  │ Channel  │    │ Channel  │    │ Channel  │            │
│  │ (cap=N)  │    │ (cap=N)  │    │ (cap=N)  │            │
│  └──────────┘    └──────────┘    └──────────┘            │
│                                                           │
│  Backpressure: when channel full, upstream blocks         │
│  Cancellation: drop receiver → sender gets SendError      │
└─────────────────────────────────────────────────────────┘
```

### 2.2 Core Trait: StreamingComputeFunction

```
┌─────────────────────────────────────────────────┐
│           StreamingComputeFunction               │
│                                                   │
│  fn stream_execute(                               │
│      &self,                                       │
│      inputs: Vec<Receiver<Chunk>>,  ← streaming   │
│      params: &HashMap<String, String>,            │
│  ) -> Receiver<Chunk>              → streaming    │
│                                                   │
│  fn supports_streaming(&self) -> bool             │
│  fn preferred_chunk_size(&self) -> usize          │
│                                                   │
│  // Fallback: batch execute still available       │
│  fn execute(&self, inputs, params) -> Bytes       │
└─────────────────────────────────────────────────┘
```

### 2.3 Chunk Type

```
┌──────────────────────────────────┐
│           StreamChunk             │
│                                   │
│  Data(Bytes)     ← payload chunk  │
│  End             ← stream done    │
│  Error(DerivaError) ← failure     │
│                                   │
│  Ordering: Data* End | Error      │
│  (zero or more Data, then End     │
│   or Error — never both)          │
└──────────────────────────────────┘
```

### 2.4 Pipeline Construction

```
  Recipe Z = f3(f1(A, B), f2(C))

  Batch execution (current):
    1. materialize A → Bytes
    2. materialize B → Bytes
    3. execute f1(A, B) → Bytes
    4. materialize C → Bytes
    5. execute f2(C) → Bytes
    6. execute f3(f1_result, f2_result) → Bytes

  Streaming execution:
    1. spawn stream_source(A) → Receiver<Chunk>
    2. spawn stream_source(B) → Receiver<Chunk>
    3. spawn f1.stream_execute([rx_A, rx_B]) → Receiver<Chunk>
    4. spawn stream_source(C) → Receiver<Chunk>
    5. spawn f2.stream_execute([rx_C]) → Receiver<Chunk>
    6. spawn f3.stream_execute([rx_f1, rx_f2]) → Receiver<Chunk>
    7. consume rx_f3 → stream to client

  All stages run concurrently via tokio tasks.
  Backpressure propagates upstream through bounded channels.
```

### 2.5 Hybrid Mode: Streaming + Batch Functions

Not all functions support streaming. The pipeline handles mixed mode:

```
  Recipe: Z = streaming_f(batch_g(A), C)

  batch_g does NOT support streaming:
    1. Materialize A fully → Bytes
    2. Execute batch_g(A) → Bytes
    3. Wrap result as single-chunk stream: [Data(result), End]

  streaming_f DOES support streaming:
    4. streaming_f receives:
         input[0] = single-chunk stream from batch_g
         input[1] = chunked stream from C
    5. streaming_f processes chunks as they arrive

  The pipeline adapter automatically wraps batch results as streams.
```

### 2.6 Backpressure Design

```
Channel capacity = N chunks (configurable, default = 8)

  Producer (upstream)          Consumer (downstream)
  ┌──────────┐                 ┌──────────┐
  │ Produces  │                │ Consumes  │
  │ chunk     │──▶ [||||||||] ──▶│ chunk     │
  │           │    channel(8)  │           │
  └──────────┘                 └──────────┘

  When channel is full:
    Producer's send() blocks (async await)
    → upstream naturally slows down
    → memory bounded to N × chunk_size per stage

  When consumer is slow:
    All upstream stages eventually block
    → no unbounded buffering
    → memory usage is predictable

  Total memory for pipeline with D stages:
    D × N × chunk_size
    Example: 5 stages × 8 chunks × 64KB = 2.5MB
```

### 2.7 Caching Strategy for Streaming Results

Streaming results need special caching treatment:

```
Option A: Cache-after-collect (chosen)
  - Collect all chunks into Bytes after stream completes
  - Cache the full Bytes value
  - Future get() returns cached Bytes (no re-streaming)
  - Pro: simple, compatible with existing cache
  - Con: must buffer full result for caching

Option B: Cache individual chunks
  - Cache each chunk separately with sequence numbers
  - Future get() replays chunks from cache
  - Pro: no full-result buffering
  - Con: complex cache management, fragmentation

Option C: No caching for streaming results
  - Always recompute streaming functions
  - Pro: zero memory overhead
  - Con: defeats the purpose of caching

Decision: Option A. The cache stores full Bytes values (existing design).
Streaming is a compute optimization, not a storage optimization. After
streaming completes, the result is collected and cached normally.
```

---

## 3. Implementation

### 3.1 StreamChunk Type

Location: `crates/deriva-core/src/streaming.rs`

```rust
use bytes::Bytes;
use crate::DerivaError;

/// A chunk in a streaming computation pipeline.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// A data chunk. May be any size, but typically `preferred_chunk_size`.
    Data(Bytes),
    /// End of stream. No more chunks will follow.
    End,
    /// Stream error. No more chunks will follow.
    Error(DerivaError),
}

impl StreamChunk {
    /// Returns true if this is a data chunk.
    pub fn is_data(&self) -> bool {
        matches!(self, StreamChunk::Data(_))
    }

    /// Returns true if this is the end-of-stream marker.
    pub fn is_end(&self) -> bool {
        matches!(self, StreamChunk::End)
    }

    /// Returns true if this is an error.
    pub fn is_error(&self) -> bool {
        matches!(self, StreamChunk::Error(_))
    }

    /// Returns the data bytes, or None if not a data chunk.
    pub fn into_data(self) -> Option<Bytes> {
        match self {
            StreamChunk::Data(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the data length, or 0 if not a data chunk.
    pub fn data_len(&self) -> usize {
        match self {
            StreamChunk::Data(b) => b.len(),
            _ => 0,
        }
    }
}
```

### 3.2 StreamingComputeFunction Trait

Location: `crates/deriva-compute/src/streaming.rs`

```rust
use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::streaming::StreamChunk;
use deriva_core::DerivaError;

/// Default chunk size for streaming: 64KB
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Default channel capacity (number of chunks buffered)
pub const DEFAULT_CHANNEL_CAPACITY: usize = 8;

/// A compute function that supports streaming input and output.
///
/// Functions implementing this trait process data incrementally,
/// reading input chunks and producing output chunks without
/// requiring the full input to be in memory.
///
/// # Contract
/// - Input receivers will yield zero or more `Data` chunks followed
///   by exactly one `End` or `Error`.
/// - The returned receiver must follow the same protocol.
/// - If any input yields `Error`, the function should propagate
///   the error and terminate.
#[async_trait::async_trait]
pub trait StreamingComputeFunction: Send + Sync {
    /// Process streaming inputs and produce streaming output.
    ///
    /// Each element in `inputs` is a receiver for one input stream.
    /// The function should spawn internal tasks as needed to process
    /// inputs concurrently.
    async fn stream_execute(
        &self,
        inputs: Vec<mpsc::Receiver<StreamChunk>>,
        params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk>;

    /// Whether this function supports streaming.
    /// If false, the pipeline will fall back to batch execution.
    fn supports_streaming(&self) -> bool {
        true
    }

    /// Preferred chunk size in bytes. The pipeline will attempt to
    /// produce chunks of this size, but may produce smaller chunks
    /// (especially the last chunk).
    fn preferred_chunk_size(&self) -> usize {
        DEFAULT_CHUNK_SIZE
    }

    /// Channel capacity for this function's output.
    fn channel_capacity(&self) -> usize {
        DEFAULT_CHANNEL_CAPACITY
    }
}
```

### 3.3 Batch-to-Stream Adapter

Location: `crates/deriva-compute/src/streaming.rs`

```rust
/// Wraps a batch `ComputeFunction` result as a single-chunk stream.
///
/// Used when a pipeline stage does not support streaming — the batch
/// result is wrapped as [Data(full_result), End].
pub fn batch_to_stream(
    data: Bytes,
    chunk_size: usize,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        // Split into chunks to maintain consistent chunk protocol
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk_size).min(data.len());
            let chunk = data.slice(offset..end);
            if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                return; // receiver dropped — cancelled
            }
            offset = end;
        }
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

/// Wraps a leaf/blob value as a chunked stream.
///
/// Used to stream leaf data into the first stage of a pipeline.
pub fn value_to_stream(
    data: Bytes,
    chunk_size: usize,
    capacity: usize,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk_size).min(data.len());
            let chunk = data.slice(offset..end);
            if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                return;
            }
            offset = end;
        }
        let _ = tx.send(StreamChunk::End).await;
    });
    rx
}

/// Collects a stream back into a single Bytes value.
///
/// Used after streaming completes to cache the full result.
pub async fn collect_stream(
    mut rx: mpsc::Receiver<StreamChunk>,
) -> Result<Bytes, DerivaError> {
    let mut parts: Vec<Bytes> = Vec::new();
    let mut total_len = 0usize;

    loop {
        match rx.recv().await {
            Some(StreamChunk::Data(chunk)) => {
                total_len += chunk.len();
                parts.push(chunk);
            }
            Some(StreamChunk::End) => break,
            Some(StreamChunk::Error(e)) => return Err(e),
            None => {
                // Channel closed without End — treat as error
                return Err(DerivaError::Compute(
                    "stream closed without End marker".into(),
                ));
            }
        }
    }

    // Concatenate all parts into a single Bytes
    if parts.len() == 1 {
        Ok(parts.into_iter().next().unwrap())
    } else {
        let mut buf = Vec::with_capacity(total_len);
        for part in parts {
            buf.extend_from_slice(&part);
        }
        Ok(Bytes::from(buf))
    }
}
```

### 3.4 StreamPipeline Builder

Location: `crates/deriva-compute/src/pipeline.rs`

```rust
use std::collections::HashMap;
use bytes::Bytes;
use tokio::sync::mpsc;
use deriva_core::{CAddr, DerivaError};
use deriva_core::streaming::StreamChunk;
use crate::streaming::{
    StreamingComputeFunction, batch_to_stream, value_to_stream,
    collect_stream, DEFAULT_CHUNK_SIZE, DEFAULT_CHANNEL_CAPACITY,
};
use crate::ComputeFunction;

/// Configuration for a streaming pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Default chunk size for stages that don't specify one.
    pub chunk_size: usize,
    /// Default channel capacity for inter-stage channels.
    pub channel_capacity: usize,
    /// Whether to cache intermediate results.
    pub cache_intermediates: bool,
    /// Maximum total memory budget for the pipeline (0 = unlimited).
    pub memory_budget: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            cache_intermediates: true,
            memory_budget: 0,
        }
    }
}

/// A node in the streaming pipeline DAG.
enum PipelineNode {
    /// A leaf/blob source — streams raw data.
    Source {
        addr: CAddr,
        data: Bytes,
    },
    /// A cached value — streams from cache.
    Cached {
        addr: CAddr,
        data: Bytes,
    },
    /// A streaming compute stage.
    StreamingStage {
        addr: CAddr,
        function: Box<dyn StreamingComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>, // indices into pipeline nodes
    },
    /// A batch compute stage (wrapped as streaming).
    BatchStage {
        addr: CAddr,
        function: Box<dyn ComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>,
    },
}

/// Executes a streaming pipeline for a recipe DAG.
///
/// The pipeline:
/// 1. Resolves the recipe DAG into a topological order
/// 2. For each node, determines if it supports streaming
/// 3. Spawns tokio tasks for each stage, connected by channels
/// 4. Returns a receiver for the final output stream
///
/// After consumption, the caller should collect the stream and cache it.
pub struct StreamPipeline {
    nodes: Vec<PipelineNode>,
    config: PipelineConfig,
}

impl StreamPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            nodes: Vec::new(),
            config,
        }
    }

    /// Add a source node (leaf or cached value).
    pub fn add_source(&mut self, addr: CAddr, data: Bytes) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::Source { addr, data });
        idx
    }

    /// Add a cached value node.
    pub fn add_cached(&mut self, addr: CAddr, data: Bytes) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::Cached { addr, data });
        idx
    }

    /// Add a streaming compute stage.
    pub fn add_streaming_stage(
        &mut self,
        addr: CAddr,
        function: Box<dyn StreamingComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::StreamingStage {
            addr, function, params, input_indices,
        });
        idx
    }

    /// Add a batch compute stage (will be wrapped as streaming).
    pub fn add_batch_stage(
        &mut self,
        addr: CAddr,
        function: Box<dyn ComputeFunction>,
        params: HashMap<String, String>,
        input_indices: Vec<usize>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PipelineNode::BatchStage {
            addr, function, params, input_indices,
        });
        idx
    }

    /// Execute the pipeline, returning the output stream of the last node.
    ///
    /// Spawns a tokio task for each stage. Stages are connected by
    /// bounded mpsc channels. Backpressure propagates naturally.
    pub async fn execute(
        self,
    ) -> Result<mpsc::Receiver<StreamChunk>, DerivaError> {
        let mut outputs: Vec<Option<mpsc::Receiver<StreamChunk>>> =
            Vec::with_capacity(self.nodes.len());

        for node in self.nodes {
            match node {
                PipelineNode::Source { data, .. }
                | PipelineNode::Cached { data, .. } => {
                    let rx = value_to_stream(
                        data,
                        self.config.chunk_size,
                        self.config.channel_capacity,
                    );
                    outputs.push(Some(rx));
                }

                PipelineNode::StreamingStage {
                    function, params, input_indices, ..
                } => {
                    let inputs: Vec<mpsc::Receiver<StreamChunk>> =
                        input_indices.iter().map(|&i| {
                            outputs[i].take().expect(
                                "input already consumed — DAG has fan-out \
                                 on streaming node (not supported yet)"
                            )
                        }).collect();

                    let rx = function.stream_execute(inputs, &params).await;
                    outputs.push(Some(rx));
                }

                PipelineNode::BatchStage {
                    function, params, input_indices, ..
                } => {
                    // Collect all inputs fully (batch requires full Bytes)
                    let mut input_bytes = Vec::with_capacity(input_indices.len());
                    for &i in &input_indices {
                        let rx = outputs[i].take().expect(
                            "input already consumed"
                        );
                        let bytes = collect_stream(rx).await?;
                        input_bytes.push(bytes);
                    }

                    // Execute batch function
                    let result = function.execute(&input_bytes, &params)?;

                    // Wrap result as stream
                    let rx = batch_to_stream(result, self.config.chunk_size);
                    outputs.push(Some(rx));
                }
            }
        }

        // Return the last node's output
        outputs.last_mut()
            .and_then(|o| o.take())
            .ok_or_else(|| DerivaError::Compute("empty pipeline".into()))
    }
}
```


### 3.5 Streaming Executor Integration

Location: `crates/deriva-compute/src/executor.rs` — add streaming path

The existing `Executor` is synchronous and recursive. The streaming path
is a separate async method that builds a `StreamPipeline` from the recipe
DAG and executes it.

```rust
use crate::pipeline::{StreamPipeline, PipelineConfig};
use crate::streaming::{StreamingComputeFunction, collect_stream};
use crate::FunctionRegistry;
use deriva_core::{CAddr, Recipe, DerivaError};
use deriva_core::streaming::StreamChunk;
use tokio::sync::mpsc;

/// Extended executor with streaming support.
///
/// Wraps the existing sync Executor and adds an async streaming path.
/// The streaming path is used when:
/// 1. The root recipe's function supports streaming
/// 2. The caller requests streaming output
///
/// Otherwise, falls back to the batch Executor.
pub struct StreamingExecutor {
    pub config: PipelineConfig,
}

impl StreamingExecutor {
    pub fn new(config: PipelineConfig) -> Self {
        Self { config }
    }

    /// Build and execute a streaming pipeline for the given recipe.
    ///
    /// Returns a receiver that yields output chunks. The caller is
    /// responsible for collecting and caching the result.
    ///
    /// # Pipeline Construction Algorithm
    ///
    /// 1. Topologically sort the recipe DAG (reuse DagStore::resolve_order)
    /// 2. For each addr in topo order:
    ///    a. If cached → add as Cached source node
    ///    b. If leaf (in blob store) → add as Source node
    ///    c. If recipe with streaming function → add StreamingStage
    ///    d. If recipe with batch-only function → add BatchStage
    /// 3. Execute the pipeline
    pub async fn materialize_streaming<C, L>(
        &self,
        addr: &CAddr,
        dag: &deriva_core::DagStore,
        cache: &C,
        leaf_store: &L,
        registry: &FunctionRegistry,
    ) -> Result<mpsc::Receiver<StreamChunk>, DerivaError>
    where
        C: MaterializationCache,
        L: LeafStore,
    {
        // Step 1: Get topological order
        let topo_order = dag.resolve_order(addr)?;

        // Step 2: Build pipeline
        let mut pipeline = StreamPipeline::new(self.config.clone());
        let mut addr_to_idx: std::collections::HashMap<CAddr, usize> =
            std::collections::HashMap::new();

        for topo_addr in &topo_order {
            // Check cache first
            if let Some(cached_data) = cache.get(topo_addr) {
                let idx = pipeline.add_cached(*topo_addr, cached_data);
                addr_to_idx.insert(*topo_addr, idx);
                continue;
            }

            // Check leaf store
            if let Some(leaf_data) = leaf_store.get(topo_addr)? {
                let idx = pipeline.add_source(*topo_addr, leaf_data);
                addr_to_idx.insert(*topo_addr, idx);
                continue;
            }

            // Must be a recipe — look up function
            let recipe = dag.get_recipe(topo_addr)
                .ok_or_else(|| DerivaError::NotFound(
                    format!("recipe not found: {:?}", topo_addr)
                ))?;

            let input_indices: Vec<usize> = recipe.inputs.iter()
                .map(|input_addr| {
                    *addr_to_idx.get(input_addr).expect(
                        "topo sort guarantees inputs are processed first"
                    )
                })
                .collect();

            // Check if function supports streaming
            let func_key = format!(
                "{}:{}", recipe.function_name, recipe.function_version
            );

            if let Some(streaming_fn) =
                registry.get_streaming(&func_key)
            {
                let idx = pipeline.add_streaming_stage(
                    *topo_addr,
                    streaming_fn,
                    recipe.params.clone(),
                    input_indices,
                );
                addr_to_idx.insert(*topo_addr, idx);
            } else if let Some(batch_fn) = registry.get(&func_key) {
                let idx = pipeline.add_batch_stage(
                    *topo_addr,
                    batch_fn,
                    recipe.params.clone(),
                    input_indices,
                );
                addr_to_idx.insert(*topo_addr, idx);
            } else {
                return Err(DerivaError::Compute(
                    format!("function not found: {}", func_key)
                ));
            }
        }

        // Step 3: Execute pipeline
        pipeline.execute().await
    }
}
```

### 3.6 FunctionRegistry Extension

Location: `crates/deriva-compute/src/lib.rs` — extend registry

```rust
use crate::streaming::StreamingComputeFunction;

/// Extended FunctionRegistry that supports both batch and streaming functions.
impl FunctionRegistry {
    /// Register a streaming compute function.
    pub fn register_streaming(
        &mut self,
        name: &str,
        version: &str,
        function: Box<dyn StreamingComputeFunction>,
    ) {
        let key = format!("{}:{}", name, version);
        self.streaming_functions.insert(key, function);
    }

    /// Look up a streaming function by key.
    pub fn get_streaming(
        &self,
        key: &str,
    ) -> Option<Box<dyn StreamingComputeFunction>> {
        // Note: returns a clone/Arc — streaming functions must be
        // cheaply cloneable (typically stateless)
        self.streaming_functions.get(key).cloned()
    }

    /// Check if a function has a streaming implementation.
    pub fn has_streaming(&self, key: &str) -> bool {
        self.streaming_functions.contains_key(key)
    }
}

// Updated struct:
pub struct FunctionRegistry {
    functions: HashMap<String, Arc<dyn ComputeFunction>>,
    streaming_functions: HashMap<String, Arc<dyn StreamingComputeFunction>>,
}
```

### 3.7 Built-in Streaming Functions

Location: `crates/deriva-compute/src/builtins_streaming.rs`

20 built-in streaming functions organized by pattern:

#### Category 1: Single-Input Chunk-by-Chunk Transforms

These emit one output chunk per input chunk — true streaming with no buffering.

```rust
/// 1. Streaming identity: passes input chunks through unchanged.
pub struct StreamingIdentity;

#[async_trait]
impl StreamingComputeFunction for StreamingIdentity {
    async fn stream_execute(
        &self,
        mut inputs: Vec<mpsc::Receiver<StreamChunk>>,
        _params: &HashMap<String, String>,
    ) -> mpsc::Receiver<StreamChunk> {
        assert_eq!(inputs.len(), 1, "identity takes exactly 1 input");
        inputs.remove(0)
    }
}

/// 2. Streaming uppercase: converts each chunk to uppercase ASCII.
pub struct StreamingUppercase;
// Spawns task: for each Data chunk, map bytes to ascii_uppercase, forward.

/// 3. Streaming lowercase: converts each chunk to lowercase ASCII.
pub struct StreamingLowercase;
// Same pattern as uppercase, uses to_ascii_lowercase.

/// 4. Streaming reverse: reverses bytes within each chunk.
pub struct StreamingReverse;
// For each Data chunk, reverse the byte slice, forward.

/// 5. Streaming base64 encode: encodes each chunk to base64.
pub struct StreamingBase64Encode;
// Uses base64::engine::general_purpose::STANDARD.encode(&chunk).

/// 6. Streaming base64 decode: decodes each chunk from base64.
pub struct StreamingBase64Decode;
// Uses base64::engine::general_purpose::STANDARD.decode(&chunk).
// Emits Error on invalid base64.

/// 7. Streaming XOR cipher: XORs each byte with a key from params["key"].
pub struct StreamingXor;
// Reads params["key"] as single byte. XORs each byte in each chunk.

/// 8. Streaming compress: zlib-compresses each chunk independently.
pub struct StreamingCompress;
// Uses flate2::write::ZlibEncoder per chunk. Each chunk compressed independently.

/// 9. Streaming decompress: zlib-decompresses each chunk independently.
pub struct StreamingDecompress;
// Uses flate2::write::ZlibDecoder per chunk. Each chunk decompressed independently.
```

#### Category 2: Single-Input Accumulators

These consume all input before emitting a summary result. They benefit from
streaming input because they don't buffer the full input — just update state.

```rust
/// 10. Streaming SHA-256: rolling hash, emits 32-byte digest.
pub struct StreamingSha256;
// Feeds each chunk to sha2::Sha256 hasher. On End, emits digest + End.

/// 11. Streaming byte count: counts total bytes, emits u64 as 8-byte BE.
pub struct StreamingByteCount;
// Accumulates chunk.len() for each Data. On End, emits u64::to_be_bytes() + End.

/// 12. Streaming CRC32 checksum: rolling CRC32, emits 4-byte checksum.
pub struct StreamingChecksum;
// Uses crc32fast::Hasher. Feeds each chunk. On End, emits u32::to_be_bytes() + End.
```

#### Category 3: Multi-Input Combiners

These consume multiple input streams and produce a single output stream.

```rust
/// 13. Streaming concatenation: reads inputs sequentially, emits chunks as they arrive.
pub struct StreamingConcat;
// For each input receiver in order: forward all Data chunks, on End move to next.
// After all inputs consumed, emit End.

/// 14. Streaming interleave: round-robin chunks from N inputs.
pub struct StreamingInterleave;
// Cycles through inputs: take one chunk from input[0], then input[1], etc.
// When an input ends, skip it. When all ended, emit End.

/// 15. Streaming zip-concat: pair-wise concatenate chunks from 2 inputs.
pub struct StreamingZipConcat;
// Takes exactly 2 inputs. Reads one chunk from each, concatenates them
// into a single output chunk. When either ends, drain the other.
```

#### Category 4: Pipeline Utilities

These reshape or limit streams without transforming content.

```rust
/// 16. Streaming chunk resizer: re-chunks to a target chunk size.
pub struct StreamingChunkResizer;
// Reads params["target_size"] (default 64KB). Buffers incoming chunks
// in a BytesMut. When buffer >= target_size, emit a chunk of exactly
// target_size. On End, flush remaining buffer + End.

/// 17. Streaming take: emits only the first N bytes, then End.
pub struct StreamingTake;
// Reads params["bytes"] as usize. Forwards Data chunks, tracking total
// bytes emitted. When limit reached, truncate last chunk and emit End.
// Drops remaining input (backpressure will stop upstream).

/// 18. Streaming skip: skips the first N bytes, then passes through.
pub struct StreamingSkip;
// Reads params["bytes"] as usize. Drops bytes until N consumed,
// then forwards remaining chunks unchanged.

/// 19. Streaming repeat: repeats the input stream N times.
pub struct StreamingRepeat;
// Reads params["count"] as usize. Collects full input into buffer,
// then emits the buffer N times as chunked streams. End after all repeats.

/// 20. Streaming tee-count: passes data through while counting bytes.
pub struct StreamingTeeCount;
// Forwards all Data chunks unchanged. On End, appends one final Data
// chunk containing the byte count as decimal ASCII string, then End.
// Useful for adding a trailer with metadata.
```

#### Summary Table

| # | Name | Inputs | Pattern | Crate Dep |
|---|------|--------|---------|-----------|
| 1 | StreamingIdentity | 1 | passthrough | — |
| 2 | StreamingUppercase | 1 | chunk transform | — |
| 3 | StreamingLowercase | 1 | chunk transform | — |
| 4 | StreamingReverse | 1 | chunk transform | — |
| 5 | StreamingBase64Encode | 1 | chunk transform | base64 |
| 6 | StreamingBase64Decode | 1 | chunk transform | base64 |
| 7 | StreamingXor | 1 | chunk transform | — |
| 8 | StreamingCompress | 1 | chunk transform | flate2 |
| 9 | StreamingDecompress | 1 | chunk transform | flate2 |
| 10 | StreamingSha256 | 1 | accumulator | sha2 |
| 11 | StreamingByteCount | 1 | accumulator | — |
| 12 | StreamingChecksum | 1 | accumulator | crc32fast |
| 13 | StreamingConcat | N | combiner | — |
| 14 | StreamingInterleave | N | combiner | — |
| 15 | StreamingZipConcat | 2 | combiner | — |
| 16 | StreamingChunkResizer | 1 | utility | — |
| 17 | StreamingTake | 1 | utility | — |
| 18 | StreamingSkip | 1 | utility | — |
| 19 | StreamingRepeat | 1 | utility (buffered) | — |
| 20 | StreamingTeeCount | 1 | utility | — |

#### New Dependencies

```toml
# In workspace Cargo.toml [workspace.dependencies]
sha2 = "0.10"
base64 = "0.22"
flate2 = "1"
crc32fast = "1"

# In deriva-compute/Cargo.toml [dependencies]
sha2 = { workspace = true }
base64 = { workspace = true }
flate2 = { workspace = true }
crc32fast = { workspace = true }
```


### 3.8 gRPC Streaming Integration

Location: `crates/deriva-server/src/service.rs` — update `get` RPC

```rust
use deriva_compute::pipeline::PipelineConfig;
use deriva_compute::executor::StreamingExecutor;
use deriva_compute::streaming::collect_stream as collect_compute_stream;

/// Updated get() RPC with streaming materialization support.
///
/// Decision logic:
/// 1. If value is cached → stream from cache (existing behavior)
/// 2. If root function supports streaming → use StreamPipeline
/// 3. Otherwise → batch materialize, then stream result
async fn get(
    &self,
    request: Request<GetRequest>,
) -> Result<Response<Self::GetStream>, Status> {
    let addr = parse_addr(&request.get_ref().addr)?;

    // Check cache first
    {
        let cache = self.state.cache.read()
            .map_err(|_| Status::internal("cache lock"))?;
        if let Some(data) = cache.get(&addr) {
            return Ok(Response::new(stream_bytes(data)));
        }
    }

    // Determine if streaming is available for this recipe
    let use_streaming = {
        let dag = self.state.dag.read()
            .map_err(|_| Status::internal("dag lock"))?;
        if let Some(recipe) = dag.get_recipe(&addr) {
            let key = format!(
                "{}:{}", recipe.function_name, recipe.function_version
            );
            self.state.registry.has_streaming(&key)
        } else {
            false
        }
    };

    if use_streaming {
        // Streaming path
        let streaming_executor = StreamingExecutor::new(
            PipelineConfig::default()
        );

        let dag = self.state.dag.read()
            .map_err(|_| Status::internal("dag lock"))?;

        let compute_rx = streaming_executor.materialize_streaming(
            &addr, &dag, &*self.state.cache_reader(),
            &*self.state.leaf_store, &self.state.registry,
        ).await.map_err(|e| Status::internal(e.to_string()))?;

        // Fork the stream: one path goes to client, other collects for cache
        let (client_tx, client_rx) = mpsc::channel(8);
        let (cache_tx, cache_rx) = mpsc::channel(8);
        let cache_clone = self.state.shared_cache.clone();
        let cache_addr = addr;

        tokio::spawn(async move {
            // Tee: forward chunks to both client and cache collector
            let mut compute_rx = compute_rx;
            loop {
                match compute_rx.recv().await {
                    Some(StreamChunk::Data(chunk)) => {
                        let grpc_chunk = GetResponse {
                            chunk: chunk.to_vec(),
                        };
                        let _ = client_tx.send(Ok(grpc_chunk)).await;
                        let _ = cache_tx.send(StreamChunk::Data(chunk)).await;
                    }
                    Some(StreamChunk::End) => {
                        let _ = cache_tx.send(StreamChunk::End).await;
                        break;
                    }
                    Some(StreamChunk::Error(e)) => {
                        let _ = client_tx.send(Err(
                            Status::internal(e.to_string())
                        )).await;
                        let _ = cache_tx.send(StreamChunk::Error(e)).await;
                        break;
                    }
                    None => break,
                }
            }
        });

        // Background task: collect stream and cache the result
        tokio::spawn(async move {
            if let Ok(full_data) = collect_compute_stream(cache_rx).await {
                cache_clone.put(&cache_addr, full_data).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(client_rx)))
    } else {
        // Batch path (existing behavior)
        let data = self.batch_materialize(&addr).await?;
        Ok(Response::new(stream_bytes(data)))
    }
}

/// Helper: stream a Bytes value as 64KB gRPC chunks.
fn stream_bytes(data: Bytes) -> ReceiverStream<Result<GetResponse, Status>> {
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        for chunk in data.chunks(65536) {
            if tx.send(Ok(GetResponse {
                chunk: chunk.to_vec(),
            })).await.is_err() {
                break;
            }
        }
    });
    ReceiverStream::new(rx)
}
```

### 3.9 Stream Tee Utility

Location: `crates/deriva-compute/src/streaming.rs` — add tee function

```rust
/// Tee a stream into two receivers.
///
/// Each chunk is cloned and sent to both outputs. If either receiver
/// is dropped, the tee continues sending to the other.
///
/// Used to simultaneously stream to the client and collect for caching.
pub fn tee_stream(
    mut input: mpsc::Receiver<StreamChunk>,
    capacity: usize,
) -> (mpsc::Receiver<StreamChunk>, mpsc::Receiver<StreamChunk>) {
    let (tx_a, rx_a) = mpsc::channel(capacity);
    let (tx_b, rx_b) = mpsc::channel(capacity);

    tokio::spawn(async move {
        loop {
            match input.recv().await {
                Some(chunk) => {
                    let chunk_clone = chunk.clone();
                    // Send to both; ignore errors (receiver may be dropped)
                    let _ = tx_a.send(chunk).await;
                    let _ = tx_b.send(chunk_clone).await;
                }
                None => break,
            }
        }
    });

    (rx_a, rx_b)
}
```


---

## 4. Data Flow Diagrams

### 4.1 Simple Streaming Pipeline

```
Recipe: Z = uppercase(concat(A, B))

  ┌──────┐  chunks   ┌──────────┐  chunks   ┌───────────┐  chunks
  │Leaf A │─────────▶│  concat  │─────────▶│ uppercase │─────────▶ client
  └──────┘           │(streaming)│           │(streaming)│
  ┌──────┐  chunks   │          │           │           │
  │Leaf B │─────────▶│          │           │           │
  └──────┘           └──────────┘           └───────────┘

  Timeline:
    T0: Leaf A starts streaming chunks [a1, a2, a3, End]
    T0: Leaf B starts streaming chunks [b1, b2, End]
    T1: concat receives a1, forwards to uppercase
    T1: uppercase receives a1, emits UPPER(a1) to client
    T2: concat receives a2, forwards
    T2: uppercase emits UPPER(a2) to client
    ...
    T5: concat receives b2, forwards
    T6: uppercase emits UPPER(b2), then End
    T6: Client has received all chunks

  Memory at any point: ~3 × 8 × 64KB = 1.5MB (3 channels × 8 capacity)
  vs batch: full A + full B + concat result + uppercase result
```

### 4.2 Mixed Streaming + Batch Pipeline

```
Recipe: Z = streaming_f(batch_g(A), C)

  ┌──────┐  chunks   ┌─────────┐  collect   ┌─────────┐
  │Leaf A │─────────▶│ collect │──────────▶│ batch_g │
  └──────┘           │ (await) │  Bytes     │(execute)│
                      └─────────┘           └────┬────┘
                                                  │ Bytes
                                                  ▼
                                            ┌─────────┐  chunks
                                            │batch_to_│─────────┐
                                            │ stream  │         │
                                            └─────────┘         │
                                                                 ▼
  ┌──────┐  chunks                              ┌─────────────┐  chunks
  │Leaf C │────────────────────────────────────▶│streaming_f  │─────▶ client
  └──────┘                                      │             │
                                                 └─────────────┘

  batch_g blocks until all of A is collected.
  streaming_f starts processing C chunks immediately.
  When batch_g completes, its result is wrapped as a stream.
  streaming_f then processes both input streams.
```

### 4.3 Backpressure Propagation

```
  Scenario: downstream consumer is slow

  ┌──────┐         ┌──────────┐         ┌──────────┐
  │Source │──▶ ch1 ──▶│ Stage 1  │──▶ ch2 ──▶│ Stage 2  │──▶ ch3 ──▶ client
  │(fast) │  [8/8]  │ (fast)   │  [8/8]  │ (slow)   │  [8/8]
  └──────┘  FULL    └──────────┘  FULL    └──────────┘  FULL

  T0: Client reads slowly from ch3
  T1: ch3 fills up → Stage 2's send() blocks
  T2: Stage 2 stops reading from ch2 → ch2 fills up
  T3: Stage 1's send() blocks → Stage 1 stops reading from ch1
  T4: ch1 fills up → Source's send() blocks
  T5: Entire pipeline is paused, waiting for client

  When client reads one chunk:
  T6: ch3 has space → Stage 2 unblocks, processes one chunk
  T7: ch2 has space → Stage 1 unblocks, processes one chunk
  T8: ch1 has space → Source unblocks, sends one chunk
  → Pipeline resumes at client's pace
```

### 4.4 Stream Tee for Cache Collection

```
  ┌──────────────┐
  │  Pipeline     │
  │  output       │
  │  (chunks)     │
  └──────┬───────┘
          │
          ▼
    ┌─────────┐
    │   Tee   │
    └──┬───┬──┘
       │   │
       ▼   ▼
  ┌────┐  ┌──────────┐
  │gRPC│  │ Collector │
  │send│  │ (cache)   │
  │    │  │           │
  │ c1 │  │ buf=[c1,  │
  │ c2 │  │  c2, c3]  │
  │ c3 │  │           │
  │End │  │→ cache.put│
  └────┘  └──────────┘

  Both paths receive identical chunks.
  gRPC path streams to client immediately.
  Collector path buffers and caches after End.
```

### 4.5 Cancellation Flow

```
  Client disconnects mid-stream:

  T0: Pipeline running, chunks flowing
  T1: Client drops gRPC connection
  T2: gRPC sender gets SendError → tee task detects
  T3: Tee drops input receiver
  T4: Pipeline stage's send() gets SendError
  T5: Pipeline stage returns → tokio task completes
  T6: Upstream stages detect closed channel → cascade stop
  T7: All tasks cleaned up, channels dropped

  No leaked tasks, no orphaned channels.
  Cache collector also stops (receives channel close).
```

---

## 5. Test Specification

### 5.1 Unit Tests — StreamChunk

```rust
#[test]
fn test_stream_chunk_data() {
    let chunk = StreamChunk::Data(Bytes::from("hello"));
    assert!(chunk.is_data());
    assert!(!chunk.is_end());
    assert!(!chunk.is_error());
    assert_eq!(chunk.data_len(), 5);
}

#[test]
fn test_stream_chunk_end() {
    let chunk = StreamChunk::End;
    assert!(!chunk.is_data());
    assert!(chunk.is_end());
    assert_eq!(chunk.data_len(), 0);
}

#[test]
fn test_stream_chunk_into_data() {
    let chunk = StreamChunk::Data(Bytes::from("hello"));
    assert_eq!(chunk.into_data(), Some(Bytes::from("hello")));

    let end = StreamChunk::End;
    assert_eq!(end.into_data(), None);
}
```

### 5.2 Unit Tests — batch_to_stream

```rust
#[tokio::test]
async fn test_batch_to_stream_small() {
    let data = Bytes::from("hello world");
    let mut rx = batch_to_stream(data.clone(), 64 * 1024);

    // Small data → single chunk
    match rx.recv().await.unwrap() {
        StreamChunk::Data(chunk) => assert_eq!(chunk, data),
        other => panic!("expected Data, got {:?}", other),
    }
    assert!(rx.recv().await.unwrap().is_end());
}

#[tokio::test]
async fn test_batch_to_stream_chunked() {
    let data = Bytes::from(vec![0u8; 200]);
    let mut rx = batch_to_stream(data, 64); // 64-byte chunks

    let mut total = 0;
    let mut chunk_count = 0;
    loop {
        match rx.recv().await.unwrap() {
            StreamChunk::Data(chunk) => {
                assert!(chunk.len() <= 64);
                total += chunk.len();
                chunk_count += 1;
            }
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("error: {:?}", e),
        }
    }
    assert_eq!(total, 200);
    assert_eq!(chunk_count, 4); // ceil(200/64) = 4
}

#[tokio::test]
async fn test_batch_to_stream_empty() {
    let data = Bytes::new();
    let mut rx = batch_to_stream(data, 64);
    // Empty data → just End
    assert!(rx.recv().await.unwrap().is_end());
}
```

### 5.3 Unit Tests — collect_stream

```rust
#[tokio::test]
async fn test_collect_stream_basic() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello "))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from("world"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));
}

#[tokio::test]
async fn test_collect_stream_single_chunk() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let result = collect_stream(rx).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[tokio::test]
async fn test_collect_stream_error() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("partial"))).await.unwrap();
    tx.send(StreamChunk::Error(
        DerivaError::Compute("boom".into())
    )).await.unwrap();
    drop(tx);

    let result = collect_stream(rx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_collect_stream_channel_closed() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("partial"))).await.unwrap();
    drop(tx); // close without End

    let result = collect_stream(rx).await;
    assert!(result.is_err());
}
```

### 5.4 Unit Tests — StreamingConcat

```rust
#[tokio::test]
async fn test_streaming_concat_two_inputs() {
    let concat = StreamingConcat;

    let (tx_a, rx_a) = mpsc::channel(8);
    let (tx_b, rx_b) = mpsc::channel(8);

    tx_a.send(StreamChunk::Data(Bytes::from("hello "))).await.unwrap();
    tx_a.send(StreamChunk::End).await.unwrap();
    tx_b.send(StreamChunk::Data(Bytes::from("world"))).await.unwrap();
    tx_b.send(StreamChunk::End).await.unwrap();

    let output_rx = concat.stream_execute(
        vec![rx_a, rx_b], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));
}

#[tokio::test]
async fn test_streaming_concat_empty_input() {
    let concat = StreamingConcat;

    let (tx_a, rx_a) = mpsc::channel(8);
    tx_a.send(StreamChunk::End).await.unwrap();

    let output_rx = concat.stream_execute(
        vec![rx_a], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result, Bytes::new());
}

#[tokio::test]
async fn test_streaming_concat_error_propagation() {
    let concat = StreamingConcat;

    let (tx_a, rx_a) = mpsc::channel(8);
    tx_a.send(StreamChunk::Data(Bytes::from("ok"))).await.unwrap();
    tx_a.send(StreamChunk::Error(
        DerivaError::Compute("fail".into())
    )).await.unwrap();

    let output_rx = concat.stream_execute(
        vec![rx_a], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await;
    assert!(result.is_err());
}
```

### 5.5 Unit Tests — StreamingSha256

```rust
#[tokio::test]
async fn test_streaming_sha256() {
    let sha = StreamingSha256;

    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();

    let output_rx = sha.stream_execute(
        vec![rx], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result.len(), 32); // SHA-256 = 32 bytes

    // Verify against known hash
    use sha2::{Sha256, Digest};
    let expected = Sha256::digest(b"hello");
    assert_eq!(&result[..], &expected[..]);
}

#[tokio::test]
async fn test_streaming_sha256_chunked_input() {
    let sha = StreamingSha256;

    let (tx, rx) = mpsc::channel(8);
    // Send "hello" in two chunks
    tx.send(StreamChunk::Data(Bytes::from("hel"))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from("lo"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();

    let output_rx = sha.stream_execute(
        vec![rx], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await.unwrap();

    // Same hash regardless of chunking
    use sha2::{Sha256, Digest};
    let expected = Sha256::digest(b"hello");
    assert_eq!(&result[..], &expected[..]);
}
```

### 5.6 Unit Tests — StreamingUppercase

```rust
#[tokio::test]
async fn test_streaming_uppercase() {
    let upper = StreamingUppercase;

    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from(" world"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();

    let output_rx = upper.stream_execute(
        vec![rx], &HashMap::new()
    ).await;

    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result, Bytes::from("HELLO WORLD"));
}
```

### 5.7 Unit Tests — tee_stream

```rust
#[tokio::test]
async fn test_tee_stream() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("hello"))).await.unwrap();
    tx.send(StreamChunk::Data(Bytes::from(" world"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let (rx_a, rx_b) = tee_stream(rx, 8);

    let result_a = collect_stream(rx_a).await.unwrap();
    let result_b = collect_stream(rx_b).await.unwrap();

    assert_eq!(result_a, Bytes::from("hello world"));
    assert_eq!(result_b, Bytes::from("hello world"));
}

#[tokio::test]
async fn test_tee_stream_one_receiver_dropped() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(StreamChunk::Data(Bytes::from("data"))).await.unwrap();
    tx.send(StreamChunk::End).await.unwrap();
    drop(tx);

    let (rx_a, rx_b) = tee_stream(rx, 8);
    drop(rx_b); // drop one receiver

    // Other receiver should still work
    let result_a = collect_stream(rx_a).await.unwrap();
    assert_eq!(result_a, Bytes::from("data"));
}
```

### 5.8 Integration Tests — Pipeline

```rust
#[tokio::test]
async fn test_pipeline_streaming_concat_then_uppercase() {
    // Pipeline: uppercase(concat(A, B))
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 5, // small chunks for testing
        ..Default::default()
    });

    let a = test_addr("a");
    let b = test_addr("b");
    let concat_addr = test_addr("concat_ab");
    let upper_addr = test_addr("upper");

    let idx_a = pipeline.add_source(a, Bytes::from("hello "));
    let idx_b = pipeline.add_source(b, Bytes::from("world"));
    let idx_concat = pipeline.add_streaming_stage(
        concat_addr,
        Box::new(StreamingConcat),
        HashMap::new(),
        vec![idx_a, idx_b],
    );
    let _idx_upper = pipeline.add_streaming_stage(
        upper_addr,
        Box::new(StreamingUppercase),
        HashMap::new(),
        vec![idx_concat],
    );

    let output_rx = pipeline.execute().await.unwrap();
    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result, Bytes::from("HELLO WORLD"));
}

#[tokio::test]
async fn test_pipeline_large_data_backpressure() {
    // 1MB of data through identity with small channel
    let data = Bytes::from(vec![b'x'; 1_000_000]);
    let mut pipeline = StreamPipeline::new(PipelineConfig {
        chunk_size: 1024,       // 1KB chunks
        channel_capacity: 2,    // very small buffer
        ..Default::default()
    });

    let a = test_addr("a");
    let id_addr = test_addr("id");

    let idx_a = pipeline.add_source(a, data.clone());
    let _idx_id = pipeline.add_streaming_stage(
        id_addr,
        Box::new(StreamingIdentity),
        HashMap::new(),
        vec![idx_a],
    );

    let output_rx = pipeline.execute().await.unwrap();
    let result = collect_stream(output_rx).await.unwrap();
    assert_eq!(result.len(), 1_000_000);
    assert_eq!(result, data);
}
```



### 5.9 Unit Tests — StreamingIdentity

```rust
#[tokio::test]
async fn test_streaming_identity() {
    let result = run_one(&StreamingIdentity, vec![b"abc", b"def"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("abcdef"));
}
```

### 5.10 Unit Tests — StreamingLowercase

```rust
#[tokio::test]
async fn test_streaming_lowercase() {
    let result = run_one(&StreamingLowercase, vec![b"HELLO", b" WORLD"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("hello world"));
}
```

### 5.11 Unit Tests — StreamingReverse

```rust
#[tokio::test]
async fn test_streaming_reverse() {
    let result = run_one(&StreamingReverse, vec![b"abc"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("cba"));
}

#[tokio::test]
async fn test_streaming_reverse_multi_chunk() {
    // Each chunk reversed independently
    let result = run_one(&StreamingReverse, vec![b"ab", b"cd"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("badc"));
}
```

### 5.12 Unit Tests — StreamingBase64Encode / Decode

```rust
#[tokio::test]
async fn test_streaming_base64_encode() {
    let result = run_one(&StreamingBase64Encode, vec![b"hello"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("aGVsbG8="));
}

#[tokio::test]
async fn test_streaming_base64_decode() {
    let result = run_one(&StreamingBase64Decode, vec![b"aGVsbG8="], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[tokio::test]
async fn test_streaming_base64_roundtrip() {
    // encode then decode returns original
    let encoded = run_one(&StreamingBase64Encode, vec![b"test data"], &HashMap::new()).await.unwrap();
    let decoded = run_one(&StreamingBase64Decode, vec![&encoded], &HashMap::new()).await.unwrap();
    assert_eq!(decoded, Bytes::from("test data"));
}

#[tokio::test]
async fn test_streaming_base64_decode_invalid() {
    let result = run_one(&StreamingBase64Decode, vec![b"!!!invalid!!!"], &HashMap::new()).await;
    assert!(result.is_err());
}
```

### 5.13 Unit Tests — StreamingXor

```rust
#[tokio::test]
async fn test_streaming_xor() {
    let mut params = HashMap::new();
    params.insert("key".into(), "255".into()); // 0xFF
    let result = run_one(&StreamingXor, vec![b"\x00\x01\x02"], &params).await.unwrap();
    assert_eq!(&result[..], &[0xFF, 0xFE, 0xFD]);
}

#[tokio::test]
async fn test_streaming_xor_roundtrip() {
    // XOR is its own inverse
    let mut params = HashMap::new();
    params.insert("key".into(), "42".into());
    let encrypted = run_one(&StreamingXor, vec![b"hello"], &params).await.unwrap();
    let decrypted = run_one(&StreamingXor, vec![&encrypted], &params).await.unwrap();
    assert_eq!(decrypted, Bytes::from("hello"));
}
```

### 5.14 Unit Tests — StreamingCompress / Decompress

```rust
#[tokio::test]
async fn test_streaming_compress_decompress() {
    let original = Bytes::from("hello world hello world hello world");
    let compressed = run_one(&StreamingCompress, vec![&original], &HashMap::new()).await.unwrap();
    assert_ne!(compressed, original);
    let decompressed = run_one(&StreamingDecompress, vec![&compressed], &HashMap::new()).await.unwrap();
    assert_eq!(decompressed, original);
}
```

### 5.15 Unit Tests — StreamingByteCount

```rust
#[tokio::test]
async fn test_streaming_byte_count() {
    let result = run_one(&StreamingByteCount, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    let count = u64::from_be_bytes(result[..8].try_into().unwrap());
    assert_eq!(count, 11);
}

#[tokio::test]
async fn test_streaming_byte_count_empty() {
    let result = run_one(&StreamingByteCount, vec![], &HashMap::new()).await.unwrap();
    let count = u64::from_be_bytes(result[..8].try_into().unwrap());
    assert_eq!(count, 0);
}
```

### 5.16 Unit Tests — StreamingChecksum

```rust
#[tokio::test]
async fn test_streaming_checksum() {
    let result = run_one(&StreamingChecksum, vec![b"hello"], &HashMap::new()).await.unwrap();
    assert_eq!(result.len(), 4);
    let mut h = crc32fast::Hasher::new();
    h.update(b"hello");
    assert_eq!(&result[..], &h.finalize().to_be_bytes());
}

#[tokio::test]
async fn test_streaming_checksum_chunked() {
    // Same CRC32 regardless of chunking
    let r1 = run_one(&StreamingChecksum, vec![b"hello world"], &HashMap::new()).await.unwrap();
    let r2 = run_one(&StreamingChecksum, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    assert_eq!(r1, r2);
}
```

### 5.17 Unit Tests — StreamingInterleave

```rust
#[tokio::test]
async fn test_streaming_interleave() {
    // Round-robin: a1, b1, a2, b2, a3
    let rx_a = make_stream(vec![b"a1", b"a2", b"a3"]).await;
    let rx_b = make_stream(vec![b"b1", b"b2"]).await;
    let out = StreamingInterleave.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("a1b1a2b2a3"));
}

#[tokio::test]
async fn test_streaming_interleave_single() {
    let rx = make_stream(vec![b"only"]).await;
    let out = StreamingInterleave.stream_execute(vec![rx], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("only"));
}
```

### 5.18 Unit Tests — StreamingZipConcat

```rust
#[tokio::test]
async fn test_streaming_zip_concat() {
    // Pairs: aa+11, bb+22
    let rx_a = make_stream(vec![b"aa", b"bb"]).await;
    let rx_b = make_stream(vec![b"11", b"22"]).await;
    let out = StreamingZipConcat.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("aa11bb22"));
}

#[tokio::test]
async fn test_streaming_zip_concat_uneven() {
    // Pair: aa+11, then drain remainder: bb, cc
    let rx_a = make_stream(vec![b"aa", b"bb", b"cc"]).await;
    let rx_b = make_stream(vec![b"11"]).await;
    let out = StreamingZipConcat.stream_execute(vec![rx_a, rx_b], &HashMap::new()).await;
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("aa11bbcc"));
}
```

### 5.19 Unit Tests — StreamingChunkResizer

```rust
#[tokio::test]
async fn test_streaming_chunk_resizer() {
    let mut params = HashMap::new();
    params.insert("target_size".into(), "4".into());
    // 10 bytes → re-chunked to 4, 4, 2
    let rx = make_stream(vec![b"0123456789"]).await;
    let mut out = StreamingChunkResizer.stream_execute(vec![rx], &params).await;
    let mut chunks = vec![];
    loop {
        match out.recv().await.unwrap() {
            StreamChunk::Data(c) => chunks.push(c),
            StreamChunk::End => break,
            StreamChunk::Error(e) => panic!("{:?}", e),
        }
    }
    assert_eq!(chunks, vec![
        Bytes::from("0123"), Bytes::from("4567"), Bytes::from("89")
    ]);
}
```

### 5.20 Unit Tests — StreamingTake

```rust
#[tokio::test]
async fn test_streaming_take() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "5".into());
    let result = run_one(&StreamingTake, vec![b"hello world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("hello"));
}

#[tokio::test]
async fn test_streaming_take_multi_chunk() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "7".into());
    let result = run_one(&StreamingTake, vec![b"hello", b" world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("hello w"));
}

#[tokio::test]
async fn test_streaming_take_more_than_available() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "100".into());
    let result = run_one(&StreamingTake, vec![b"short"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("short"));
}
```

### 5.21 Unit Tests — StreamingSkip

```rust
#[tokio::test]
async fn test_streaming_skip() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "6".into());
    let result = run_one(&StreamingSkip, vec![b"hello world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("world"));
}

#[tokio::test]
async fn test_streaming_skip_multi_chunk() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "3".into());
    let result = run_one(&StreamingSkip, vec![b"he", b"llo world"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("lo world"));
}

#[tokio::test]
async fn test_streaming_skip_more_than_available() {
    let mut params = HashMap::new();
    params.insert("bytes".into(), "100".into());
    let result = run_one(&StreamingSkip, vec![b"short"], &params).await.unwrap();
    assert_eq!(result, Bytes::new());
}
```

### 5.22 Unit Tests — StreamingRepeat

```rust
#[tokio::test]
async fn test_streaming_repeat() {
    let mut params = HashMap::new();
    params.insert("count".into(), "3".into());
    let result = run_one(&StreamingRepeat, vec![b"ab"], &params).await.unwrap();
    assert_eq!(result, Bytes::from("ababab"));
}

#[tokio::test]
async fn test_streaming_repeat_zero() {
    let mut params = HashMap::new();
    params.insert("count".into(), "0".into());
    let result = run_one(&StreamingRepeat, vec![b"ab"], &params).await.unwrap();
    assert_eq!(result, Bytes::new());
}
```

### 5.23 Unit Tests — StreamingTeeCount

```rust
#[tokio::test]
async fn test_streaming_tee_count() {
    // Passes data through, appends byte count as ASCII trailer
    let result = run_one(&StreamingTeeCount, vec![b"hello", b" world"], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("hello world11"));
}

#[tokio::test]
async fn test_streaming_tee_count_empty() {
    let result = run_one(&StreamingTeeCount, vec![], &HashMap::new()).await.unwrap();
    assert_eq!(result, Bytes::from("0"));
}
```

### 5.24 Integration Tests — New Function Pipelines

```rust
#[tokio::test]
async fn test_pipeline_compress_then_decompress() {
    // Pipeline: decompress(compress(src)) == src
    let mut pipeline = StreamPipeline::new(PipelineConfig::default());
    let idx_src = pipeline.add_source(test_addr("src"), Bytes::from("hello world"));
    let idx_comp = pipeline.add_streaming_stage(
        test_addr("comp"), Arc::new(StreamingCompress), HashMap::new(), vec![idx_src],
    );
    let _idx_dec = pipeline.add_streaming_stage(
        test_addr("dec"), Arc::new(StreamingDecompress), HashMap::new(), vec![idx_comp],
    );
    let out = pipeline.execute().await.unwrap();
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("hello world"));
}

#[tokio::test]
async fn test_pipeline_take_skip_compose() {
    // skip(3) then take(5) on "hello world" → "lo wo"
    let mut pipeline = StreamPipeline::new(PipelineConfig { chunk_size: 4, ..Default::default() });
    let idx_src = pipeline.add_source(test_addr("src"), Bytes::from("hello world"));
    let idx_skip = pipeline.add_streaming_stage(
        test_addr("skip"), Arc::new(StreamingSkip),
        HashMap::from([("bytes".into(), "3".into())]), vec![idx_src],
    );
    let _idx_take = pipeline.add_streaming_stage(
        test_addr("take"), Arc::new(StreamingTake),
        HashMap::from([("bytes".into(), "5".into())]), vec![idx_skip],
    );
    let out = pipeline.execute().await.unwrap();
    assert_eq!(collect_stream(out).await.unwrap(), Bytes::from("lo wo"));
}
```

#### Test Summary Table

| Section | Tests | Functions Covered |
|---------|-------|-------------------|
| §5.1 | 3 | StreamChunk type |
| §5.2 | 3 | batch_to_stream |
| §5.3 | 4 | collect_stream |
| §5.4 | 3 | StreamingConcat |
| §5.5 | 2 | StreamingSha256 |
| §5.6 | 1 | StreamingUppercase |
| §5.7 | 2 | tee_stream |
| §5.8 | 2 | Pipeline integration (original) |
| §5.9 | 1 | StreamingIdentity |
| §5.10 | 1 | StreamingLowercase |
| §5.11 | 2 | StreamingReverse |
| §5.12 | 4 | StreamingBase64Encode/Decode |
| §5.13 | 2 | StreamingXor |
| §5.14 | 1 | StreamingCompress/Decompress |
| §5.15 | 2 | StreamingByteCount |
| §5.16 | 2 | StreamingChecksum |
| §5.17 | 2 | StreamingInterleave |
| §5.18 | 2 | StreamingZipConcat |
| §5.19 | 1 | StreamingChunkResizer |
| §5.20 | 3 | StreamingTake |
| §5.21 | 3 | StreamingSkip |
| §5.22 | 2 | StreamingRepeat |
| §5.23 | 2 | StreamingTeeCount |
| §5.24 | 2 | Pipeline integration (new) |
| **Total** | **52** | **20 functions + core utilities** |

---

## 6. Edge Cases & Error Handling

| Case | Behavior | Rationale |
|------|----------|-----------|
| Function doesn't support streaming | Falls back to batch execution via `BatchStage` | Hybrid mode — streaming is opt-in per function |
| Input stream yields Error | Error propagated to output stream, pipeline stops | Fail-fast — no point continuing with partial data |
| Client disconnects mid-stream | Channel SendError detected, all tasks stop | Graceful cancellation via channel drop |
| Empty input (0 bytes) | Stream yields End immediately, result is empty Bytes | Valid — empty data is a valid value |
| Single-byte chunks | Works correctly, just slower | Chunk size is a hint, not a requirement |
| Very large single chunk (>1GB) | Works but defeats streaming purpose | Chunk size is advisory — functions may emit any size |
| Pipeline with no stages | Returns error "empty pipeline" | Defensive check in `StreamPipeline::execute` |
| Fan-out (one output used by two stages) | Currently panics ("input already consumed") | Limitation — requires stream cloning (future work) |
| Recursive/cyclic recipe | Impossible — DAG enforced by `DagStore::insert` | Topo sort would fail on cycles |
| Channel capacity = 0 | `mpsc::channel(0)` is invalid in tokio | Minimum capacity is 1; default is 8 |
| All stages are batch | Pipeline works, just no streaming benefit | Batch stages collect inputs and execute normally |
| Cache hit on intermediate node | Intermediate served from cache as Source | Avoids recomputing upstream stages |

### 6.1 Fan-Out Limitation

```
Current limitation:
  Recipe: Z = f(X, X)  — same input used twice

  Pipeline tries to take X's receiver twice → panic

Solution (future work):
  Detect fan-out during pipeline construction.
  Use tee_stream() to clone the output for multiple consumers.

  let (rx_x_1, rx_x_2) = tee_stream(rx_x, capacity);
  // rx_x_1 → first input of f
  // rx_x_2 → second input of f

  For now: fan-out falls back to batch execution for the
  duplicated input (collect once, wrap as two streams).
```

### 6.2 Memory Budget Enforcement

```
PipelineConfig.memory_budget = 10MB

  Total buffered = stages × channel_capacity × chunk_size
  Example: 5 stages × 8 × 64KB = 2.5MB → OK

  If budget exceeded:
    Reduce channel_capacity proportionally
    new_capacity = budget / (stages × chunk_size)
    Example: 10MB / (20 stages × 64KB) = 8 → OK
    Example: 1MB / (20 stages × 64KB) = 0.78 → clamp to 1

  This is advisory — actual memory may exceed budget due to
  in-flight chunks being processed by stages.
```

---

## 7. Performance Analysis

### 7.1 Throughput Comparison

```
Benchmark: 100MB data through 3-stage pipeline

Batch mode:
  Stage 1: 100MB in, 100MB out → 200ms
  Stage 2: 100MB in, 100MB out → 200ms
  Stage 3: 100MB in, 100MB out → 200ms
  Total: 600ms (sequential)
  Peak memory: 200MB (input + output of current stage)

Streaming mode (64KB chunks, capacity=8):
  All stages run concurrently
  Pipeline latency ≈ max(200ms, 200ms, 200ms) = 200ms
  Plus pipeline fill time: ~1ms
  Total: ~201ms (3× faster)
  Peak memory: 3 × 8 × 64KB = 1.5MB (1000× less)
```

### 7.2 Latency to First Byte

```
Batch mode:
  First byte available after ALL computation completes
  100MB through 3 stages: 600ms to first byte

Streaming mode:
  First byte available after first chunk through pipeline
  64KB through 3 stages: ~0.1ms to first byte
  6000× improvement in time-to-first-byte
```

### 7.3 Channel Overhead

```
Per-chunk overhead:
  mpsc::send: ~50ns (uncontended)
  mpsc::recv: ~50ns (uncontended)
  Total per stage: ~100ns per chunk

For 100MB with 64KB chunks = 1600 chunks:
  Channel overhead per stage: 1600 × 100ns = 160μs
  3 stages: 480μs total channel overhead
  Negligible compared to compute time (200ms)

Conclusion: channel overhead is <0.1% of total time
```

### 7.4 Optimal Chunk Size

```
Chunk size trade-offs:

  Too small (1KB):
    + Low memory usage
    - High channel overhead (100K sends for 100MB)
    - Poor CPU cache utilization
    - More context switches

  Too large (10MB):
    + Low channel overhead
    + Good CPU cache utilization
    - High memory usage (stages × capacity × 10MB)
    - Poor pipeline parallelism (few chunks in flight)

  Sweet spot (64KB):
    ✓ Fits in L2 cache
    ✓ ~1600 chunks for 100MB (manageable overhead)
    ✓ Memory: 8 × 64KB = 512KB per channel
    ✓ Good pipeline parallelism

  Recommendation: 64KB default, configurable per function
```

### 7.5 Benchmark Plan

```rust
#[bench]
fn bench_streaming_vs_batch_1mb(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = Bytes::from(vec![0u8; 1_000_000]);

    b.iter(|| {
        rt.block_on(async {
            // Streaming: source → identity → collect
            let mut pipeline = StreamPipeline::new(PipelineConfig::default());
            let idx = pipeline.add_source(test_addr("a"), data.clone());
            pipeline.add_streaming_stage(
                test_addr("id"), Box::new(StreamingIdentity),
                HashMap::new(), vec![idx],
            );
            let rx = pipeline.execute().await.unwrap();
            collect_stream(rx).await.unwrap()
        })
    });
}

#[bench]
fn bench_streaming_vs_batch_100mb(b: &mut Bencher) {
    // Same as above with 100MB data
}

#[bench]
fn bench_streaming_3_stage_pipeline(b: &mut Bencher) {
    // concat → uppercase → sha256
}

#[bench]
fn bench_backpressure_slow_consumer(b: &mut Bencher) {
    // Consumer adds 1ms delay per chunk
}

#[bench]
fn bench_chunk_size_comparison(b: &mut Bencher) {
    // Compare 1KB, 16KB, 64KB, 256KB, 1MB chunk sizes
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-core/src/streaming.rs` | **NEW** — `StreamChunk` enum |
| `deriva-core/src/lib.rs` | Add `pub mod streaming` |
| `deriva-compute/src/streaming.rs` | **NEW** — `StreamingComputeFunction` trait, adapters, `collect_stream`, `tee_stream` |
| `deriva-compute/src/pipeline.rs` | **NEW** — `StreamPipeline`, `PipelineConfig`, `PipelineNode` |
| `deriva-compute/src/builtins_streaming.rs` | **NEW** — `StreamingIdentity`, `StreamingConcat`, `StreamingSha256`, `StreamingUppercase` |
| `deriva-compute/src/executor.rs` | Add `StreamingExecutor` with `materialize_streaming()` |
| `deriva-compute/src/lib.rs` | Add `pub mod streaming`, `pub mod pipeline`, `pub mod builtins_streaming`; extend `FunctionRegistry` |
| `deriva-server/src/service.rs` | Update `get()` RPC with streaming path, add `stream_bytes()` helper |
| `proto/deriva.proto` | No changes (streaming is transparent to proto — `get` already streams) |
| `deriva-compute/tests/streaming.rs` | **NEW** — unit tests for StreamChunk, adapters |
| `deriva-compute/tests/builtins_streaming.rs` | **NEW** — unit tests for built-in streaming functions |
| `deriva-compute/tests/pipeline.rs` | **NEW** — integration tests for StreamPipeline |

---

## 9. Dependency Changes

| Crate | Dependency | Version | Reason |
|-------|-----------|---------|--------|
| `deriva-compute` | `async-trait` | `0.1` | `StreamingComputeFunction` is an async trait |
| `deriva-compute` | `sha2` | `0.10` | `StreamingSha256` built-in (already a dependency) |

`tokio::sync::mpsc` is already available via the `tokio` dependency.
`bytes::Bytes` is already available via the `bytes` dependency.

---

## 10. Design Rationale

### 10.1 Why mpsc Channels Instead of Streams/AsyncIterator?

| Factor | mpsc channels | tokio Stream | async Iterator |
|--------|--------------|-------------|----------------|
| Backpressure | Built-in (bounded) | Requires wrapper | Manual |
| Cancellation | Drop receiver | Drop stream | Drop iterator |
| Multi-producer | Supported | Complex | No |
| Spawning tasks | Natural (send tx to task) | Requires pinning | Requires pinning |
| Debugging | Channel capacity visible | Opaque | Opaque |

mpsc channels are the simplest primitive that provides backpressure and
cancellation. The `StreamingComputeFunction` trait uses `Receiver<StreamChunk>`
directly — no abstraction layer needed.

### 10.2 Why Cache-After-Collect Instead of Chunk Caching?

Chunk caching would require:
1. New cache data structure (ordered chunk list per CAddr)
2. Sequence numbers on chunks
3. Cache eviction per-chunk or per-stream
4. Replay logic for cache hits

This complexity is not justified because:
- The cache already stores `Bytes` values efficiently
- Streaming is a compute optimization, not a storage optimization
- After streaming completes, the full result is known and cacheable
- Cache hits bypass streaming entirely (serve from cache)

### 10.3 Why Not Make All Functions Streaming?

Streaming adds complexity to function implementations:
- Must handle chunk boundaries (data may split at any byte)
- Must manage internal state across chunks
- Must handle End and Error signals
- Testing is more complex (async, channels)

Many functions are naturally batch (e.g., sort, aggregate, compress).
Forcing streaming on them would require buffering all input anyway,
adding overhead without benefit.

The hybrid approach lets each function choose the best mode.

### 10.4 Why Separate StreamingComputeFunction Trait?

Alternative: add `stream_execute` to existing `ComputeFunction` trait.

Problem: `ComputeFunction` is `Send + Sync` but not async. Adding an
async method would require `async_trait` on the base trait, breaking
all existing implementations.

Separate trait allows:
- Existing batch functions unchanged
- Streaming opt-in per function
- Registry stores both types independently
- Clear separation of concerns

### 10.5 Why Default Chunk Size of 64KB?

```
Analysis of common CPU cache sizes:
  L1: 32-64KB per core
  L2: 256KB-1MB per core
  L3: 8-32MB shared

64KB fits in L1/L2 cache, enabling:
  - Single-pass processing without cache misses
  - Efficient SIMD operations on chunk data
  - Reasonable channel overhead (~1600 sends per 100MB)

Smaller (4KB): too many sends, poor throughput
Larger (1MB): doesn't fit L1, higher memory per channel
64KB: sweet spot for most workloads
```

---

## 11. Observability Integration

New metrics for streaming pipelines (integrates with §2.5):

```rust
lazy_static! {
    static ref STREAM_PIPELINES_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_pipelines_total",
        "Total streaming pipelines executed"
    ).unwrap();

    static ref STREAM_CHUNKS_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_chunks_total",
        "Total chunks processed across all pipelines"
    ).unwrap();

    static ref STREAM_BYTES_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_bytes_total",
        "Total bytes processed through streaming pipelines"
    ).unwrap();

    static ref STREAM_PIPELINE_DURATION: Histogram = register_histogram!(
        "deriva_stream_pipeline_duration_seconds",
        "End-to-end streaming pipeline duration",
        vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 30.0]
    ).unwrap();

    static ref STREAM_BACKPRESSURE_WAITS: IntCounterVec = register_int_counter_vec!(
        "deriva_stream_backpressure_waits_total",
        "Times a stage blocked on full channel",
        &["stage"]
    ).unwrap();
}
```

---

## 12. Checklist

- [ ] Create `deriva-core/src/streaming.rs` with `StreamChunk`
- [ ] Create `deriva-compute/src/streaming.rs` with `StreamingComputeFunction` trait
- [ ] Implement `batch_to_stream`, `value_to_stream`, `collect_stream`, `tee_stream`
- [ ] Create `deriva-compute/src/pipeline.rs` with `StreamPipeline`
- [ ] Implement `PipelineConfig` with defaults
- [ ] Create `deriva-compute/src/builtins_streaming.rs` with 4 built-in functions
- [ ] Extend `FunctionRegistry` with streaming function storage
- [ ] Add `StreamingExecutor` to `executor.rs`
- [ ] Update `get()` RPC with streaming path and cache-after-collect
- [ ] Write unit tests for `StreamChunk` (~3 tests)
- [ ] Write unit tests for adapters (~7 tests)
- [ ] Write unit tests for built-in streaming functions (~8 tests)
- [ ] Write unit tests for `tee_stream` (~2 tests)
- [ ] Write integration tests for `StreamPipeline` (~2 tests)
- [ ] Run benchmarks: streaming vs batch at 1MB, 100MB
- [ ] Run benchmarks: chunk size comparison
- [ ] Verify backpressure works with slow consumer test
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing 244+ tests still pass
- [ ] Commit and push
