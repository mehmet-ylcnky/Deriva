use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_log::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- SyslogParseFn (#383) ----
#[test]
fn syslog_parse_rfc5424() {
    let log = b"<165>1 2024-01-15T10:30:00Z myhost myapp 1234 ID47 - Application started\n";
    let r = SyslogParseFn.execute(vec![Bytes::from(&log[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["hostname"], "myhost");
    assert_eq!(line["app_name"], "myapp");
    assert_eq!(line["priority"], 165);
    assert_eq!(line["severity"], 165 % 8);
}
#[test]
fn syslog_parse_rfc3164() {
    let log = b"<34>Jan 15 10:30:00 myhost su: pam_unix(su:session): session opened\n";
    let r = SyslogParseFn.execute(vec![Bytes::from(&log[..])], &p(&[("format","rfc3164")])).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["hostname"], "myhost");
    assert_eq!(line["priority"], 34);
}
#[test]
fn syslog_parse_malformed_line() {
    let log = b"this is not syslog\n";
    let r = SyslogParseFn.execute(vec![Bytes::from(&log[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["parse_error"], true);
}
#[test]
fn syslog_parse_structured_data() {
    let log = b"<165>1 2024-01-15T10:30:00Z host app 1 ID [exampleSDID@32473 iut=\"3\"] msg\n";
    let r = SyslogParseFn.execute(vec![Bytes::from(&log[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert!(!line["structured_data"].is_null());
}
#[test]
fn syslog_parse_no_structured_data() {
    let log = b"<165>1 2024-01-15T10:30:00Z host app 1 ID - message here\n";
    let r = SyslogParseFn.execute(vec![Bytes::from(&log[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert!(line["structured_data"].is_null());
}

// ---- SyslogWriteFn (#384) ----
#[test]
fn syslog_write_rfc5424() {
    let ndjson = r#"{"priority":165,"version":"1","timestamp":"2024-01-15T10:30:00Z","hostname":"myhost","app_name":"myapp","procid":"1234","msgid":"ID47","structured_data":null,"message":"test"}"#;
    let r = SyslogWriteFn.execute(vec![Bytes::from(ndjson)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("<165>1"));
    assert!(text.contains("myhost"));
}
#[test]
fn syslog_write_rfc3164() {
    let ndjson = r#"{"priority":34,"timestamp":"Jan 15 10:30:00","hostname":"myhost","message":"test msg"}"#;
    let r = SyslogWriteFn.execute(vec![Bytes::from(ndjson)], &p(&[("format","rfc3164")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("<34>"));
    assert!(text.contains("myhost"));
}
#[test]
fn syslog_write_parse_roundtrip() {
    let log = "<165>1 2024-01-15T10:30:00Z myhost myapp 1234 ID47 - Application started";
    let parsed = SyslogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let written = SyslogWriteFn.execute(vec![parsed], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&written).unwrap();
    assert!(text.contains("<165>"));
    assert!(text.contains("myhost"));
    assert!(text.contains("Application started"));
}
#[test]
fn syslog_write_defaults() {
    let ndjson = r#"{"message":"hello"}"#;
    let r = SyslogWriteFn.execute(vec![Bytes::from(ndjson)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("<13>")); // default priority
}
#[test]
fn syslog_write_invalid_json_error() {
    let r = SyslogWriteFn.execute(vec![Bytes::from("not json")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- CefParseFn (#385) ----
#[test]
fn cef_parse_basic() {
    let cef = b"CEF:0|Security|IDS|1.0|100|Attack detected|7|src=10.0.0.1 dst=10.0.0.2\n";
    let r = CefParseFn.execute(vec![Bytes::from(&cef[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["vendor"], "Security");
    assert_eq!(line["severity"], "7");
    assert_eq!(line["extensions"]["src"], "10.0.0.1");
}
#[test]
fn cef_parse_no_extensions() {
    let cef = b"CEF:0|Vendor|Product|1.0|100|Name|5|\n";
    let r = CefParseFn.execute(vec![Bytes::from(&cef[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["vendor"], "Vendor");
}
#[test]
fn cef_parse_multiple_lines() {
    let cef = b"CEF:0|V|P|1|1|N1|3|k1=v1\nCEF:0|V|P|1|2|N2|5|k2=v2\n";
    let r = CefParseFn.execute(vec![Bytes::from(&cef[..])], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn cef_parse_signature_id() {
    let cef = b"CEF:0|V|P|1.0|SIG123|Event|3|\n";
    let r = CefParseFn.execute(vec![Bytes::from(&cef[..])], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["signature_id"], "SIG123");
}
#[test]
fn cef_parse_empty_input() {
    let r = CefParseFn.execute(vec![Bytes::from("")], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}
