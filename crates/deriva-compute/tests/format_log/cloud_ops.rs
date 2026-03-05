use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_log::*;
use std::collections::BTreeMap;

// ---- CloudTrailParseFn (#389) ----
#[test]
fn cloudtrail_parse_basic() {
    let ct = r#"{"Records":[{"eventName":"ConsoleLogin","sourceIPAddress":"1.2.3.4"},{"eventName":"StopInstances"}]}"#;
    let r = CloudTrailParseFn.execute(vec![Bytes::from(ct)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 2);
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["eventName"], "ConsoleLogin");
}
#[test]
fn cloudtrail_parse_single_record() {
    let ct = r#"{"Records":[{"eventName":"CreateBucket","requestParameters":{"bucketName":"test"}}]}"#;
    let r = CloudTrailParseFn.execute(vec![Bytes::from(ct)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 1);
}
#[test]
fn cloudtrail_parse_empty_records() {
    let ct = r#"{"Records":[]}"#;
    let r = CloudTrailParseFn.execute(vec![Bytes::from(ct)], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}
#[test]
fn cloudtrail_parse_missing_records() {
    let ct = r#"{"Events":[]}"#;
    let r = CloudTrailParseFn.execute(vec![Bytes::from(ct)], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn cloudtrail_parse_invalid_json() {
    let r = CloudTrailParseFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- VpcFlowLogParseFn (#390) ----
#[test]
fn vpc_flow_log_parse_basic() {
    let flow = "version account-id interface-id srcaddr dstaddr\n2 123456789012 eni-abc 10.0.0.1 10.0.0.2\n";
    let r = VpcFlowLogParseFn.execute(vec![Bytes::from(flow)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["srcaddr"], "10.0.0.1");
    assert_eq!(line["dstaddr"], "10.0.0.2");
}
#[test]
fn vpc_flow_log_parse_multiple() {
    let flow = "srcaddr dstaddr action\n10.0.0.1 10.0.0.2 ACCEPT\n10.0.0.3 10.0.0.4 REJECT\n";
    let r = VpcFlowLogParseFn.execute(vec![Bytes::from(flow)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn vpc_flow_log_parse_custom_fields() {
    let flow = "srcaddr srcport dstaddr dstport protocol\n10.0.0.1 443 10.0.0.2 54321 6\n";
    let r = VpcFlowLogParseFn.execute(vec![Bytes::from(flow)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["srcport"], "443");
    assert_eq!(line["protocol"], "6");
}
#[test]
fn vpc_flow_log_parse_empty_body() {
    let flow = "srcaddr dstaddr\n";
    let r = VpcFlowLogParseFn.execute(vec![Bytes::from(flow)], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}
#[test]
fn vpc_flow_log_parse_empty_input() {
    let r = VpcFlowLogParseFn.execute(vec![Bytes::from("")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- PrometheusExpositionParseFn (#391) ----
#[test]
fn prometheus_parse_basic() {
    let prom = "# HELP http_requests Total requests\n# TYPE http_requests counter\nhttp_requests{method=\"GET\",code=\"200\"} 1027\n";
    let r = PrometheusExpositionParseFn.execute(vec![Bytes::from(prom)], &BTreeMap::new()).unwrap();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0]["name"], "http_requests");
    assert_eq!(metrics[0]["value"], 1027.0);
    assert_eq!(metrics[0]["labels"]["method"], "GET");
}
#[test]
fn prometheus_parse_no_labels() {
    let prom = "go_goroutines 42\n";
    let r = PrometheusExpositionParseFn.execute(vec![Bytes::from(prom)], &BTreeMap::new()).unwrap();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(metrics[0]["name"], "go_goroutines");
    assert_eq!(metrics[0]["value"], 42.0);
}
#[test]
fn prometheus_parse_with_timestamp() {
    let prom = "metric_total 100 1704067200\n";
    let r = PrometheusExpositionParseFn.execute(vec![Bytes::from(prom)], &BTreeMap::new()).unwrap();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(metrics[0]["timestamp"], "1704067200");
}
#[test]
fn prometheus_parse_multiple_metrics() {
    let prom = "a 1\nb 2\nc 3\n";
    let r = PrometheusExpositionParseFn.execute(vec![Bytes::from(prom)], &BTreeMap::new()).unwrap();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(metrics.len(), 3);
}
#[test]
fn prometheus_parse_skips_comments() {
    let prom = "# HELP a help\n# TYPE a gauge\na 5\n# another comment\nb 10\n";
    let r = PrometheusExpositionParseFn.execute(vec![Bytes::from(prom)], &BTreeMap::new()).unwrap();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(metrics.len(), 2);
}
