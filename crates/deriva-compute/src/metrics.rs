use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge, register_gauge_vec, register_histogram,
    register_histogram_vec, register_int_counter, register_int_counter_vec,
    CounterVec, Gauge, GaugeVec, Histogram, HistogramVec, IntCounter, IntCounterVec,
};

lazy_static! {
    pub static ref MAT_TOTAL: CounterVec = register_counter_vec!(
        "deriva_materialize_total", "Total materializations", &["result"]
    ).unwrap();
    pub static ref MAT_DURATION: Histogram = register_histogram!(
        "deriva_materialize_duration_seconds", "Materialization latency",
        vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0]
    ).unwrap();
    pub static ref MAT_ACTIVE: Gauge = register_gauge!(
        "deriva_materialize_active", "In-flight materializations"
    ).unwrap();
    pub static ref CACHE_TOTAL: CounterVec = register_counter_vec!(
        "deriva_cache_total", "Cache operations", &["result"]
    ).unwrap();
    pub static ref COMPUTE_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_compute_duration_seconds", "Compute function latency",
        &["function"],
        vec![0.0001, 0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]
    ).unwrap();
    pub static ref COMPUTE_INPUT_BYTES: HistogramVec = register_histogram_vec!(
        "deriva_compute_input_bytes", "Input size per compute",
        &["function"],
        vec![64.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0]
    ).unwrap();
    pub static ref COMPUTE_OUTPUT_BYTES: HistogramVec = register_histogram_vec!(
        "deriva_compute_output_bytes", "Output size per compute",
        &["function"],
        vec![64.0, 256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0]
    ).unwrap();
    pub static ref CACHE_EVICTION_TOTAL: IntCounter = register_int_counter!(
        "deriva_cache_eviction_total", "Total cache evictions"
    ).unwrap();
    pub static ref CACHE_SIZE: Gauge = register_gauge!(
        "deriva_cache_size_bytes", "Current cache size in bytes"
    ).unwrap();
    pub static ref CACHE_ENTRIES: Gauge = register_gauge!(
        "deriva_cache_entries", "Current number of cache entries"
    ).unwrap();
    pub static ref CACHE_HIT_RATE: Gauge = register_gauge!(
        "deriva_cache_hit_rate", "Rolling cache hit rate"
    ).unwrap();
}

// Streaming pipeline metrics (§2.7 §11)
lazy_static! {
    pub static ref STREAM_PIPELINES_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_pipelines_total",
        "Total streaming pipelines executed"
    ).unwrap();
    pub static ref STREAM_CHUNKS_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_chunks_total",
        "Total chunks processed across all pipelines"
    ).unwrap();
    pub static ref STREAM_BYTES_TOTAL: IntCounter = register_int_counter!(
        "deriva_stream_bytes_total",
        "Total bytes processed through streaming pipelines"
    ).unwrap();
    pub static ref STREAM_PIPELINE_DURATION: Histogram = register_histogram!(
        "deriva_stream_pipeline_duration_seconds",
        "End-to-end streaming pipeline duration",
        vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 30.0]
    ).unwrap();
}

// §11 Per-function streaming metrics
lazy_static! {
    pub static ref STREAMING_FN_CALLS: IntCounterVec = register_int_counter_vec!(
        "deriva_streaming_fn_calls_total",
        "Number of streaming function invocations",
        &["function"]
    ).unwrap();
    pub static ref STREAMING_FN_ERRORS: IntCounterVec = register_int_counter_vec!(
        "deriva_streaming_fn_errors_total",
        "Number of streaming function errors",
        &["function"]
    ).unwrap();
    pub static ref STREAMING_FN_DURATION: HistogramVec = register_histogram_vec!(
        "deriva_streaming_fn_duration_seconds",
        "Streaming function execution duration",
        &["function"],
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0, 60.0]
    ).unwrap();
}

// §2.9 Size-aware mode selection metrics
lazy_static! {
    pub static ref MODE_SELECTION: IntCounterVec = register_int_counter_vec!(
        "deriva_mode_selection_total",
        "Execution mode selections by mode and reason",
        &["mode", "reason"]
    ).unwrap();
    pub static ref STREAMING_THRESHOLD_GAUGE: prometheus::IntGauge = prometheus::register_int_gauge!(
        "deriva_streaming_threshold_bytes",
        "Configured streaming threshold in bytes"
    ).unwrap();
}

// §2.10 Adaptive chunk sizing metrics
lazy_static! {
    pub static ref ADAPTIVE_CHUNK_SIZE_BYTES: GaugeVec = register_gauge_vec!(
        "deriva_adaptive_chunk_size_bytes",
        "Current target chunk size per adaptive resizer",
        &["stage"]
    ).unwrap();
    pub static ref ADAPTIVE_RESIZE_EVENTS_TOTAL: IntCounterVec = register_int_counter_vec!(
        "deriva_adaptive_resize_events_total",
        "Resize event count by stage and direction",
        &["stage", "direction"]
    ).unwrap();
}
