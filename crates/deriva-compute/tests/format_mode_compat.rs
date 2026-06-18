//! Mode selection compatibility (Property 7)
//!
//! For functions registered in both batch and streaming modes,
//! verify they produce semantically equivalent output.

use bytes::Bytes;
use deriva_compute::builtins_format_csv::*;
use deriva_compute::function::ComputeFunction;
use deriva_compute::streaming::StreamingComputeFunction;
use deriva_compute::streaming::{collect_stream, value_to_stream, DEFAULT_CHUNK_SIZE};
use std::collections::BTreeMap;
use std::collections::HashMap;
use deriva_core::address::Value;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

fn sp(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[tokio::test]
async fn csv_filter_batch_stream_equivalent() {
    let csv = b"name,age\nAlice,30\nBob,25\nCharlie,35\n";
    let input = Bytes::from_static(csv);
    let batch_fn = CsvFilterFn;
    let params = p(&[("column", "age"), ("op", "gt"), ("value", "28")]);
    let sparams = sp(&[("column", "age"), ("op", "gt"), ("value", "28")]);

    let batch_result = batch_fn.execute(vec![input.clone()], &params).unwrap();

    let stream_fn = CsvFilterFn;
    let rx = value_to_stream(input, DEFAULT_CHUNK_SIZE, 8);
    let stream_rx = stream_fn.stream_execute(vec![rx], &sparams).await;
    let stream_result = collect_stream(stream_rx).await.unwrap();

    // Both should filter to rows where age > 28
    assert_eq!(batch_result, stream_result);
}

#[tokio::test]
async fn ndjson_filter_batch_stream_equivalent() {
    let ndjson = b"{\"name\":\"Alice\",\"age\":30}\n{\"name\":\"Bob\",\"age\":25}\n";
    let input = Bytes::from_static(ndjson);
    let batch_fn = NdjsonFilterFn;
    let params = p(&[("path", "age"), ("op", "gt"), ("value", "28")]);
    let sparams = sp(&[("path", "age"), ("op", "gt"), ("value", "28")]);

    let batch_result = batch_fn.execute(vec![input.clone()], &params).unwrap();

    let rx = value_to_stream(input, DEFAULT_CHUNK_SIZE, 8);
    let stream_rx = NdjsonFilterFn.stream_execute(vec![rx], &sparams).await;
    let stream_result = collect_stream(stream_rx).await.unwrap();

    assert_eq!(batch_result, stream_result);
}
