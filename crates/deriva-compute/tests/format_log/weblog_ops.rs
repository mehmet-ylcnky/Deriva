use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_log::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- ElfParseFn (#386) ----
#[test]
fn elf_parse_basic() {
    let elf = "#Fields: date time s-ip cs-method cs-uri-stem\n2024-01-15 10:30:00 10.0.0.1 GET /index.html\n";
    let r = ElfParseFn.execute(vec![Bytes::from(elf)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["date"], "2024-01-15");
    assert_eq!(line["cs-method"], "GET");
}
#[test]
fn elf_parse_skips_comments() {
    let elf = "#Version: 1.0\n#Fields: a b\n#Date: 2024-01-15\n1 2\n";
    let r = ElfParseFn.execute(vec![Bytes::from(elf)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 1);
}
#[test]
fn elf_parse_multiple_rows() {
    let elf = "#Fields: x y\n1 2\n3 4\n5 6\n";
    let r = ElfParseFn.execute(vec![Bytes::from(elf)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 3);
}
#[test]
fn elf_parse_extra_fields() {
    let elf = "#Fields: a b\n1 2 3\n";
    let r = ElfParseFn.execute(vec![Bytes::from(elf)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["a"], "1");
    assert!(line.get("field_2").is_some());
}
#[test]
fn elf_parse_empty() {
    let r = ElfParseFn.execute(vec![Bytes::from("#Fields: a\n")], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}

// ---- ApacheLogParseFn (#387) ----
#[test]
fn apache_log_parse_combined() {
    let log = r#"192.168.1.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326 "http://www.example.com/start.html" "Mozilla/4.08""#;
    let r = ApacheLogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["remote_addr"], "192.168.1.1");
    assert_eq!(line["method"], "GET");
    assert_eq!(line["status"], "200");
}
#[test]
fn apache_log_parse_common() {
    let log = r#"127.0.0.1 - jan [10/Oct/2000:13:55:36 -0700] "POST /form HTTP/1.1" 302 0"#;
    let r = ApacheLogParseFn.execute(vec![Bytes::from(log)], &p(&[("format","common")])).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["method"], "POST");
    assert_eq!(line["status"], "302");
}
#[test]
fn apache_log_parse_multiple() {
    let log = "192.168.1.1 - - [10/Oct/2000:13:55:36 -0700] \"GET / HTTP/1.0\" 200 100 \"-\" \"Bot\"\n10.0.0.1 - - [10/Oct/2000:13:55:37 -0700] \"GET /b HTTP/1.0\" 404 0 \"-\" \"Bot\"\n";
    let r = ApacheLogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn apache_log_parse_dash_bytes() {
    let log = r#"10.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET / HTTP/1.0" 304 - "-" "Bot""#;
    let r = ApacheLogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["body_bytes"], "-");
}
#[test]
fn apache_log_parse_user_agent() {
    let log = r#"10.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET / HTTP/1.0" 200 0 "-" "Chrome/120.0""#;
    let r = ApacheLogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["user_agent"], "Chrome/120.0");
}

// ---- NginxLogParseFn (#388) ----
#[test]
fn nginx_log_parse_combined_delegates() {
    let log = r#"10.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET / HTTP/1.1" 200 612 "-" "curl/7.68""#;
    let r = NginxLogParseFn.execute(vec![Bytes::from(log)], &BTreeMap::new()).unwrap();
    let line: serde_json::Value = serde_json::from_str(std::str::from_utf8(&r).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(line["remote_addr"], "10.0.0.1");
}
#[test]
fn nginx_log_parse_json_format() {
    let log = r#"{"remote_addr":"10.0.0.1","status":"200"}"#;
    let r = NginxLogParseFn.execute(vec![Bytes::from(log)], &p(&[("format","json")])).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap(), log);
}
#[test]
fn nginx_log_parse_json_invalid() {
    let r = NginxLogParseFn.execute(vec![Bytes::from("not json")], &p(&[("format","json")]));
    assert!(r.is_err());
}
#[test]
fn nginx_log_parse_json_multiline() {
    let log = "{\"a\":1}\n{\"b\":2}\n";
    let r = NginxLogParseFn.execute(vec![Bytes::from(log)], &p(&[("format","json")])).unwrap();
    assert_eq!(std::str::from_utf8(&r).unwrap().lines().count(), 2);
}
#[test]
fn nginx_log_parse_empty() {
    let r = NginxLogParseFn.execute(vec![Bytes::from("")], &p(&[("format","json")])).unwrap();
    assert!(r.is_empty());
}
