use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum DerivaError {
    #[error("cycle detected in DAG: {0}")]
    CycleDetected(String),

    #[error("address not found: {0}")]
    NotFound(String),

    #[error("function not registered: {0}")]
    FunctionNotFound(String),

    #[error("compute failed: {0}")]
    ComputeFailed(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    /// Non-deterministic compute function detected.
    ///
    /// Returned when verification mode detects a function producing different outputs
    /// for identical inputs. This indicates a non-deterministic function that violates
    /// the computation-addressed model's reproducibility guarantee.
    ///
    /// # Common Causes
    ///
    /// - **Random number generation**: `rand::random()`, `uuid::Uuid::new_v4()`
    /// - **System time**: `SystemTime::now()`, `Instant::now()`
    /// - **Non-deterministic iteration**: `HashMap` iteration order
    /// - **Floating-point non-associativity**: `(a + b) + c != a + (b + c)`
    /// - **Concurrency**: Race conditions, thread IDs
    ///
    /// # Debugging
    ///
    /// 1. Check error message for output hashes and sizes
    /// 2. Enable verification mode: `--verification dual`
    /// 3. Add logging to suspect functions
    /// 4. Use deterministic alternatives:
    ///    - `rand::SeedableRng` instead of `rand::thread_rng()`
    ///    - `BTreeMap` instead of `HashMap` for iteration
    ///    - Pass timestamps as function parameters
    ///
    /// # Example Error
    ///
    /// ```text
    /// determinism violation for CAddr(a71479b7...): function my_func/1
    /// produced different outputs (8 bytes hash=4d067153... vs 8 bytes hash=d63bd9a8...)
    /// ```
    #[error("determinism violation for {addr}: function {function_id} produced different outputs ({output_1_len} bytes hash={output_1_hash} vs {output_2_len} bytes hash={output_2_hash})")]
    DeterminismViolation {
        addr: String,
        function_id: String,
        output_1_hash: String,
        output_2_hash: String,
        output_1_len: usize,
        output_2_len: usize,
    },
}

pub type Result<T> = std::result::Result<T, DerivaError>;
