use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_log::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- OtlpDecodeFn (#392) ----
#[test]
fn otlp_decode_json_traces() {
    let otlp = r#"{"resourceSpans":[{"resource":{"attributes":[]},"scopeSpans":[]}]}"#;
    let r = OtlpDecodeFn.execute(vec![Bytes::from(otlp)], &p(&[("signal","traces")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["resourceSpans"].is_array());
}
#[test]
fn otlp_decode_json_metrics() {
    let otlp = r#"{"resourceMetrics":[]}"#;
    let r = OtlpDecodeFn.execute(vec![Bytes::from(otlp)], &p(&[("signal","metrics")])).unwrap();
    assert!(!r.is_empty());
}
#[test]
fn otlp_decode_missing_signal() {
    let r = OtlpDecodeFn.execute(vec![Bytes::from("{}")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn otlp_decode_invalid_signal() {
    let r = OtlpDecodeFn.execute(vec![Bytes::from("{}")], &p(&[("signal","invalid")]));
    assert!(r.is_err());
}
#[test]
fn otlp_decode_protobuf_error() {
    let r = OtlpDecodeFn.execute(vec![Bytes::from(vec![0u8, 1, 2, 3])], &p(&[("signal","traces")]));
    assert!(r.is_err());
}

// ---- LogTimestampNormalizeFn (#393) ----
#[test]
fn timestamp_normalize_iso() {
    let log = r#"{"timestamp":"2024-01-15T10:30:00Z","msg":"test"}"#;
    let r = LogTimestampNormalizeFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["normalized"], true);
    assert!(line["normalized_timestamp"].as_str().unwrap().contains("2024-01-15"));
}
#[test]
fn timestamp_normalize_apache() {
    let log = r#"{"raw":"[15/Jan/2024:10:30:00 +0000] something"}"#;
    let r = LogTimestampNormalizeFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["normalized"], true);
    assert!(line["normalized_timestamp"].as_str().unwrap().contains("2024-01-15"));
}
#[test]
fn timestamp_normalize_slash_format() {
    let log = r#"{"raw":"2024/01/15 10:30:00 event"}"#;
    let r = LogTimestampNormalizeFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["normalized"], true);
    assert!(line["normalized_timestamp"].as_str().unwrap().contains("2024-01-15"));
}
#[test]
fn timestamp_normalize_no_timestamp() {
    let log = r#"{"msg":"no timestamp here"}"#;
    let r = LogTimestampNormalizeFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["normalized"], false);
}
#[test]
fn timestamp_normalize_syslog_format() {
    let log = "Jan 15 10:30:00 myhost something happened";
    let r = LogTimestampNormalizeFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["normalized"], true);
}

// ---- LogLevelFilterFn (#394) ----
#[test]
fn log_level_filter_json() {
    let logs = "{\"level\":\"debug\",\"msg\":\"d\"}\n{\"level\":\"info\",\"msg\":\"i\"}\n{\"level\":\"error\",\"msg\":\"e\"}\n";
    let r = LogLevelFilterFn.execute(vec![Bytes::from(logs)], &p(&[("min_level","warn")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("error"));
}
#[test]
fn log_level_filter_text() {
    let logs = "2024-01-15 DEBUG starting\n2024-01-15 ERROR failed\n2024-01-15 INFO running\n";
    let r = LogLevelFilterFn.execute(vec![Bytes::from(logs)], &p(&[("min_level","error")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("ERROR"));
}
#[test]
fn log_level_filter_all_pass() {
    let logs = "{\"level\":\"error\",\"msg\":\"e\"}\n{\"level\":\"fatal\",\"msg\":\"f\"}\n";
    let r = LogLevelFilterFn.execute(vec![Bytes::from(logs)], &p(&[("min_level","error")])).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn log_level_filter_default_debug() {
    let logs = "{\"level\":\"debug\",\"msg\":\"d\"}\n{\"level\":\"info\",\"msg\":\"i\"}\n";
    let r = LogLevelFilterFn.execute(vec![Bytes::from(logs)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn log_level_filter_no_level_passes() {
    let logs = "some line without level\n";
    let r = LogLevelFilterFn.execute(vec![Bytes::from(logs)], &p(&[("min_level","error")])).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 1);
}

// ---- LogAnonymizeFn (#395) ----
#[test]
fn anonymize_ip() {
    let log = "Connection from 192.168.1.100 to 10.0.0.1\n";
    let r = LogAnonymizeFn.execute(vec![Bytes::from(log)], &p(&[("fields","ip")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(!text.contains("192.168.1.100"));
    assert!(text.contains("REDACTED_IP_"));
}
#[test]
fn anonymize_email() {
    let log = "User test@example.com logged in\n";
    let r = LogAnonymizeFn.execute(vec![Bytes::from(log)], &p(&[("fields","email")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(!text.contains("test@example.com"));
    assert!(text.contains("REDACTED_EMAIL_"));
}
#[test]
fn anonymize_deterministic() {
    let log = "From 10.0.0.1 and 10.0.0.1 again\n";
    let r = LogAnonymizeFn.execute(vec![Bytes::from(log)], &p(&[("fields","ip")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let hashes: Vec<&str> = text.match_indices("REDACTED_IP_").map(|(i, _)| &text[i+12..i+20]).collect();
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes[0], hashes[1]);
}
#[test]
fn anonymize_user_json() {
    let log = r#"{"user":"alice","remote_user":"bob","msg":"test"}"#;
    let r = LogAnonymizeFn.execute(vec![Bytes::from(log)], &p(&[("fields","user")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(!text.contains("alice"));
    assert!(!text.contains("bob"));
    assert!(text.contains("REDACTED_USER_"));
}
#[test]
fn anonymize_combined() {
    let log = "10.0.0.1 user@test.com logged in\n";
    let r = LogAnonymizeFn.execute(vec![Bytes::from(log)], &p(&[("fields","ip,email")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("REDACTED_IP_"));
    assert!(text.contains("REDACTED_EMAIL_"));
}
