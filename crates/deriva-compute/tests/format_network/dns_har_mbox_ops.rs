use bytes::Bytes;
use deriva_compute::builtins_format_network::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- dns_record_parse (5 tests) ----
#[test]
fn dns_record_parse_basic() {
    let zone = "example.com. 3600 IN A 192.0.2.1\nexample.com. 3600 IN MX 10 mail.example.com.\n";
    let out = DnsRecordParseFn.execute(vec![Bytes::from(zone)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"type\":\"A\""));
    assert!(text.contains("\"type\":\"MX\""));
}

#[test]
fn dns_record_parse_comments() {
    let zone = "; Comment\nexample.com. 3600 IN A 192.0.2.1\n; Another comment\n";
    let out = DnsRecordParseFn.execute(vec![Bytes::from(zone)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn dns_record_parse_empty() {
    let out = DnsRecordParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn dns_record_parse_no_input() {
    assert!(DnsRecordParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn dns_record_parse_id() {
    assert_eq!(DnsRecordParseFn.id().name, "dns_record_parse");
}

// ---- har_parse (5 tests) ----
#[test]
fn har_parse_basic() {
    let har = r#"{"log":{"entries":[{"request":{"url":"http://example.com","method":"GET"},"response":{"status":200,"bodySize":1024},"time":100}]}}"#;
    let out = HarParseFn.execute(vec![Bytes::from(har)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"url\""));
    assert!(text.contains("\"status\":200"));
}

#[test]
fn har_parse_summary() {
    let har = r#"{"log":{"entries":[{"request":{"url":"http://example.com","method":"GET"},"response":{"status":200,"bodySize":1024},"time":100},{"request":{"url":"http://example.org","method":"POST"},"response":{"status":201,"bodySize":512},"time":50}]}}"#;
    let out = HarParseFn.execute(vec![Bytes::from(har)], &p(&[("summary", "true")])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["total_requests"], 2);
    assert_eq!(json["total_bytes"], 1536);
}

#[test]
fn har_parse_invalid() {
    assert!(HarParseFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn har_parse_no_input() {
    assert!(HarParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn har_parse_id() {
    assert_eq!(HarParseFn.id().name, "har_parse");
}

// ---- mbox_split (5 tests) ----
#[test]
fn mbox_split_basic() {
    let mbox = "From alice@example.com Mon Jan 1 00:00:00 2024\nSubject: Test\n\nBody\n\nFrom bob@example.org Tue Jan 2 00:00:00 2024\nSubject: Test2\n\nBody2\n";
    let out = MboxSplitFn.execute(vec![Bytes::from(mbox)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn mbox_split_single() {
    let mbox = "From alice@example.com Mon Jan 1 00:00:00 2024\nSubject: Test\n\nBody\n";
    let out = MboxSplitFn.execute(vec![Bytes::from(mbox)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"from\":\"alice@example.com"));
}

#[test]
fn mbox_split_empty() {
    let out = MboxSplitFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn mbox_split_no_input() {
    assert!(MboxSplitFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn mbox_split_id() {
    assert_eq!(MboxSplitFn.id().name, "mbox_split");
}
