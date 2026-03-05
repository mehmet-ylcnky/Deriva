use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_config::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

// ---- HclParseFn (#350) ----
#[test]
fn hcl_parse_attribute() {
    let hcl = b"name = \"test\"\ncount = 3\n";
    let r = HclParseFn.execute(vec![Bytes::from(&hcl[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "test");
}
#[test]
fn hcl_parse_block() {
    let hcl = b"resource \"aws_instance\" \"web\" {\n  ami = \"ami-123\"\n  instance_type = \"t2.micro\"\n}\n";
    let r = HclParseFn.execute(vec![Bytes::from(&hcl[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["resource.aws_instance.web"]["ami"].as_str().is_some());
}
#[test]
fn hcl_parse_nested_block() {
    let hcl = b"provider \"aws\" {\n  region = \"us-east-1\"\n}\n";
    let r = HclParseFn.execute(vec![Bytes::from(&hcl[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["provider.aws"]["region"], "us-east-1");
}
#[test]
fn hcl_parse_list_value() {
    let hcl = b"tags = [\"a\", \"b\", \"c\"]\n";
    let r = HclParseFn.execute(vec![Bytes::from(&hcl[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
}
#[test]
fn hcl_parse_invalid_error() {
    let r = HclParseFn.execute(vec![Bytes::from("{{{bad")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- PropertiesParseFn (#351) ----
#[test]
fn properties_parse_equals() {
    let props = b"db.host=localhost\ndb.port=5432\n";
    let r = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["db.host"], "localhost");
    assert_eq!(v["db.port"], "5432");
}
#[test]
fn properties_parse_colon_separator() {
    let props = b"key:value\n";
    let r = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["key"], "value");
}
#[test]
fn properties_parse_comments() {
    let props = b"# comment\n! another comment\nkey=value\n";
    let r = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v.as_object().unwrap().len(), 1);
    assert_eq!(v["key"], "value");
}
#[test]
fn properties_parse_line_continuation() {
    let props = b"long.key = hello \\\nworld\n";
    let r = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    let val = v["long.key"].as_str().unwrap();
    assert!(val.contains("hello") && val.contains("world"));
}
#[test]
fn properties_parse_empty_value() {
    let props = b"empty=\n";
    let r = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["empty"], "");
}

// ---- PropertiesWriteFn (#352) ----
#[test]
fn properties_write_simple() {
    let json = br#"{"db.host":"localhost","db.port":"5432"}"#;
    let r = PropertiesWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("db.host=localhost"));
    assert!(text.contains("db.port=5432"));
}
#[test]
fn properties_write_roundtrip() {
    let props = b"key=value\nname=test\n";
    let json = PropertiesParseFn.execute(vec![Bytes::from(&props[..])], &BTreeMap::new()).unwrap();
    let back = PropertiesWriteFn.execute(vec![json], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&back).unwrap();
    assert!(text.contains("key=value"));
    assert!(text.contains("name=test"));
}
#[test]
fn properties_write_numeric() {
    let json = br#"{"count":42}"#;
    let r = PropertiesWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("count=42"));
}
#[test]
fn properties_write_invalid_json_error() {
    let r = PropertiesWriteFn.execute(vec![Bytes::from("bad")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn properties_write_non_object_error() {
    let r = PropertiesWriteFn.execute(vec![Bytes::from("[1]")], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- PlistParseFn (#353) ----
#[test]
fn plist_parse_string_values() {
    let plist = b"<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n  <key>name</key>\n  <string>Alice</string>\n</dict>\n</plist>";
    let r = PlistParseFn.execute(vec![Bytes::from(&plist[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["name"], "Alice");
}
#[test]
fn plist_parse_integer() {
    let plist = b"<plist version=\"1.0\">\n<dict>\n  <key>count</key>\n  <integer>42</integer>\n</dict>\n</plist>";
    let r = PlistParseFn.execute(vec![Bytes::from(&plist[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["count"], 42);
}
#[test]
fn plist_parse_boolean() {
    let plist = b"<plist version=\"1.0\">\n<dict>\n  <key>enabled</key>\n  <true/>\n  <key>debug</key>\n  <false/>\n</dict>\n</plist>";
    let r = PlistParseFn.execute(vec![Bytes::from(&plist[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["enabled"], true);
    assert_eq!(v["debug"], false);
}
#[test]
fn plist_parse_not_plist_error() {
    let r = PlistParseFn.execute(vec![Bytes::from("<html>not plist</html>")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn plist_parse_real() {
    let plist = b"<plist version=\"1.0\">\n<dict>\n  <key>pi</key>\n  <real>3.14</real>\n</dict>\n</plist>";
    let r = PlistParseFn.execute(vec![Bytes::from(&plist[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!((v["pi"].as_f64().unwrap() - 3.14).abs() < 0.01);
}

// ---- PlistWriteFn (#354) ----
#[test]
fn plist_write_string() {
    let json = br#"{"name":"Alice"}"#;
    let r = PlistWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<key>name</key>"));
    assert!(text.contains("<string>Alice</string>"));
    assert!(text.contains("<plist"));
}
#[test]
fn plist_write_integer() {
    let json = br#"{"count":42}"#;
    let r = PlistWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<integer>42</integer>"));
}
#[test]
fn plist_write_boolean() {
    let json = br#"{"flag":true}"#;
    let r = PlistWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<true/>"));
}
#[test]
fn plist_write_roundtrip() {
    let json = br#"{"name":"test","count":5}"#;
    let plist = PlistWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let back = PlistParseFn.execute(vec![plist], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(v["name"], "test");
    assert_eq!(v["count"], 5);
}
#[test]
fn plist_write_includes_doctype() {
    let json = br#"{"a":"b"}"#;
    let r = PlistWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<!DOCTYPE plist"));
}
