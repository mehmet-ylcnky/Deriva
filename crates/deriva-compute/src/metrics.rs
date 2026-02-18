use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge, register_histogram, register_histogram_vec,
    register_int_counter,
    CounterVec, Gauge, Histogram, HistogramVec, IntCounter,
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
