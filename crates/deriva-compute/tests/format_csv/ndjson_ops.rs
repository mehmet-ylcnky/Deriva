use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_csv::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const EVENTS: &[u8] = b"{\"event\":\"login\",\"user\":\"alice\",\"ts\":1000}\n{\"event\":\"purchase\",\"user\":\"bob\",\"ts\":1001}\n{\"event\":\"logout\",\"user\":\"alice\",\"ts\":1002}\n{\"event\":\"error\",\"user\":\"charlie\",\"ts\":1003}\n";

// ---- NdjsonParseFn (#230) ----
#[test]
fn ndjson_parse_to_array() {
    let r = NdjsonParseFn.execute(vec![Bytes::from(EVENTS)], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 4);
    assert_eq!(arr[0]["event"], "login");
}
#[test]
fn ndjson_parse_skips_empty_lines() {
    let data = b"{\"a\":1}\n\n{\"a\":2}\n\n";
    let r = NdjsonParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 2);
}
#[test]
fn ndjson_parse_single_line() {
    let data = b"{\"key\":\"value\"}\n";
    let r = NdjsonParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 1);
}
#[test]
fn ndjson_parse_nested_objects() {
    let data = b"{\"user\":{\"name\":\"alice\",\"role\":\"admin\"}}\n{\"user\":{\"name\":\"bob\",\"role\":\"viewer\"}}\n";
    let r = NdjsonParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["user"]["role"], "admin");
}
#[test]
fn ndjson_parse_write_roundtrip() {
    let r = NdjsonParseFn.execute(vec![Bytes::from(EVENTS)], &BTreeMap::new()).unwrap();
    let back = NdjsonWriteFn.execute(vec![r], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&back).unwrap();
    assert_eq!(text.lines().count(), 4);
}

// ---- NdjsonWriteFn (#231) ----
#[test]
fn ndjson_write_from_array() {
    let json = r#"[{"a":1},{"a":2}]"#;
    let r = NdjsonWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 2);
}
#[test]
fn ndjson_write_preserves_types() {
    let json = r#"[{"s":"hello","n":42,"b":true}]"#;
    let r = NdjsonWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("42"));
    assert!(text.contains("true"));
}
#[test]
fn ndjson_write_empty_array() {
    let r = NdjsonWriteFn.execute(vec![Bytes::from("[]")], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}
#[test]
fn ndjson_write_single_element() {
    let json = r#"[{"x":1}]"#;
    let r = NdjsonWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 1);
}
#[test]
fn ndjson_write_nested() {
    let json = r#"[{"a":{"b":{"c":1}}}]"#;
    let r = NdjsonWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["a"]["b"]["c"], 1);
}

// ---- NdjsonFilterFn (#232) ----
#[test]
fn ndjson_filter_by_event_type() {
    let r = NdjsonFilterFn.execute(vec![Bytes::from(EVENTS)], &p(&[("path","event"),("op","eq"),("value","login")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("alice"));
}
#[test]
fn ndjson_filter_by_nested_path() {
    let data = b"{\"user\":{\"role\":\"admin\"},\"name\":\"alice\"}\n{\"user\":{\"role\":\"viewer\"},\"name\":\"bob\"}\n";
    let r = NdjsonFilterFn.execute(vec![Bytes::from(&data[..])], &p(&[("path","user.role"),("op","eq"),("value","admin")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("alice"));
}
#[test]
fn ndjson_filter_contains() {
    let r = NdjsonFilterFn.execute(vec![Bytes::from(EVENTS)], &p(&[("path","user"),("op","contains"),("value","li")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 3); // alice(login), alice(logout), charlie
}
#[test]
fn ndjson_filter_no_matches() {
    let r = NdjsonFilterFn.execute(vec![Bytes::from(EVENTS)], &p(&[("path","event"),("op","eq"),("value","delete")])).unwrap();
    assert!(r.is_empty());
}
#[test]
fn ndjson_filter_numeric_comparison() {
    let r = NdjsonFilterFn.execute(vec![Bytes::from(EVENTS)], &p(&[("path","ts"),("op","gt"),("value","1001")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert_eq!(text.lines().count(), 2); // ts=1002, ts=1003
}

// ---- NdjsonProjectFn (#233) ----
#[test]
fn ndjson_project_single_field() {
    let r = NdjsonProjectFn.execute(vec![Bytes::from(EVENTS)], &p(&[("paths","user")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("user").is_some());
        assert!(v.get("event").is_none());
    }
}
#[test]
fn ndjson_project_multiple_fields() {
    let r = NdjsonProjectFn.execute(vec![Bytes::from(EVENTS)], &p(&[("paths","event,ts")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert!(first.get("event").is_some());
    assert!(first.get("ts").is_some());
    assert!(first.get("user").is_none());
}
#[test]
fn ndjson_project_nonexistent_field_is_null() {
    let r = NdjsonProjectFn.execute(vec![Bytes::from(EVENTS)], &p(&[("paths","nonexistent")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert!(first["nonexistent"].is_null());
}
#[test]
fn ndjson_project_preserves_line_count() {
    let r = NdjsonProjectFn.execute(vec![Bytes::from(EVENTS)], &p(&[("paths","event")])).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 4);
}
#[test]
fn ndjson_project_nested_path() {
    let data = b"{\"a\":{\"b\":1}}\n{\"a\":{\"b\":2}}\n";
    let r = NdjsonProjectFn.execute(vec![Bytes::from(&data[..])], &p(&[("paths","a.b")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["a.b"], 1);
}
