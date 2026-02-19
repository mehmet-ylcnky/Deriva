use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge, register_gauge_vec,
    register_histogram, register_histogram_vec,
    CounterVec, Encoder, Gauge, GaugeVec, Histogram, HistogramVec, TextEncoder,
};

// Re-export compute-level metrics so service.rs can use them via `crate::metrics::*`
pub use deriva_compute::metrics::{
    CACHE_TOTAL, COMPUTE_DURATION, COMPUTE_INPUT_BYTES, COMPUTE_OUTPUT_BYTES,
    MAT_ACTIVE, MAT_DURATION, MAT_TOTAL,
};

lazy_static! {
    // RPC metrics
    pub static ref RPC_TOTAL: CounterVec = register_counter_vec!(
        "deriva_rpc_total", "Total RPC calls", &["method", "status"]
    ).unwrap();
    pub static ref RPC_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_rpc_duration_seconds", "RPC latency",
        &["method"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
    ).unwrap();
    pub static ref RPC_ACTIVE: GaugeVec = register_gauge_vec!(
        "deriva_rpc_active", "In-flight RPCs", &["method"]
    ).unwrap();

    // Cache gauges (set by status RPC)
    pub static ref CACHE_SIZE: Gauge = register_gauge!(
        "deriva_cache_size_bytes", "Current cache size"
    ).unwrap();
    pub static ref CACHE_ENTRIES: Gauge = register_gauge!(
        "deriva_cache_entries", "Current cache entries"
    ).unwrap();
    pub static ref CACHE_HIT_RATE: Gauge = register_gauge!(
        "deriva_cache_hit_rate", "Rolling cache hit rate"
    ).unwrap();

    // Materialization depth
    pub static ref MAT_DEPTH: Histogram = register_histogram!(
        "deriva_materialize_depth", "DAG depth of materialized addrs",
        vec![0.0, 1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 50.0, 100.0]
    ).unwrap();

    // Verification metrics
    pub static ref VERIFY_TOTAL: CounterVec = register_counter_vec!(
        "deriva_verification_total", "Verification outcomes", &["result"]
    ).unwrap();
    pub static ref VERIFY_FAILURE_RATE: Gauge = register_gauge!(
        "deriva_verification_failure_rate", "Rolling verification failure rate"
    ).unwrap();

    // DAG metrics
    pub static ref DAG_RECIPES: Gauge = register_gauge!(
        "deriva_dag_recipes", "Total recipes in DAG"
    ).unwrap();
    pub static ref DAG_INSERT_DURATION: Histogram = register_histogram!(
        "deriva_dag_insert_duration_seconds", "DAG insert latency",
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05]
    ).unwrap();

    // Storage metrics
    pub static ref STORAGE_BLOBS: Gauge = register_gauge!(
        "deriva_storage_blobs", "Total blobs"
    ).unwrap();
    pub static ref STORAGE_BLOB_BYTES: Gauge = register_gauge!(
        "deriva_storage_blob_bytes", "Total blob storage bytes"
    ).unwrap();

    // Cascade metrics
    pub static ref CASCADE_TOTAL: CounterVec = register_counter_vec!(
        "deriva_cascade_invalidation_total", "Total cascade invalidations", &["policy"]
    ).unwrap();

    // GC metrics
    pub static ref GC_RUNS_TOTAL: CounterVec = register_counter_vec!(
        "deriva_gc_runs_total", "Total GC cycles by mode", &["mode"]
    ).unwrap();
    pub static ref GC_BLOBS_REMOVED: Histogram = register_histogram!(
        "deriva_gc_blobs_removed", "Blobs removed per GC cycle",
        vec![0.0, 1.0, 10.0, 100.0, 1000.0, 10000.0]
    ).unwrap();
    pub static ref GC_BYTES_RECLAIMED: Histogram = register_histogram!(
        "deriva_gc_bytes_reclaimed", "Bytes reclaimed per GC cycle",
        vec![0.0, 1024.0, 1e6, 1e7, 1e8, 1e9]
    ).unwrap();
    pub static ref GC_DURATION: Histogram = register_histogram!(
        "deriva_gc_duration_seconds", "GC cycle duration",
        vec![0.01, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0]
    ).unwrap();
    pub static ref GC_LIVE_BLOBS: Gauge = register_gauge!(
        "deriva_gc_live_blobs", "Number of live blobs after last GC"
    ).unwrap();
    pub static ref GC_PINNED_COUNT: Gauge = register_gauge!(
        "deriva_gc_pinned_count", "Number of pinned addrs"
    ).unwrap();
    pub static ref CASCADE_EVICTED: Histogram = register_histogram!(
        "deriva_cascade_evicted_count", "Entries evicted per cascade",
        vec![0.0, 1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 10000.0]
    ).unwrap();
    pub static ref CASCADE_DEPTH: Histogram = register_histogram!(
        "deriva_cascade_max_depth", "Maximum depth reached during cascade BFS",
        vec![0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
    ).unwrap();
    pub static ref CASCADE_DURATION: Histogram = register_histogram!(
        "deriva_cascade_duration_seconds", "Cascade invalidation duration",
        vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]
    ).unwrap();
}

pub fn encode_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
