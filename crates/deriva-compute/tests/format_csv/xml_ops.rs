use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_csv::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// ---- XmlParseFn (#238) ----
#[test]
fn xml_parse_simple_document() {
    let xml = b"<root><name>Alice</name><age>30</age></root>";
    let r = XmlParseFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["root"]["name"]["#text"].as_str().unwrap() == "Alice");
}
#[test]
fn xml_parse_with_attributes() {
    let xml = b"<item id=\"42\" type=\"widget\"><name>Widget</name></item>";
    let r = XmlParseFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["item"]["@id"], "42");
    assert_eq!(v["item"]["@type"], "widget");
}
#[test]
fn xml_parse_nested_elements() {
    let xml = b"<config><db><host>localhost</host><port>5432</port></db></config>";
    let r = XmlParseFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert!(v["config"]["db"]["host"]["#text"].as_str().unwrap() == "localhost");
}
#[test]
fn xml_parse_empty_element() {
    let xml = b"<root><empty/></root>";
    let r = XmlParseFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    assert!(r.len() > 2); // valid JSON output
}
#[test]
fn xml_parse_invalid_xml_error() {
    let xml = b"<root><a>1</b></root>"; // mismatched tags
    let r = XmlParseFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new());
    assert!(r.is_err());
}

// ---- XmlWriteFn (#239) ----
#[test]
fn xml_write_simple_object() {
    let json = br#"{"name":"Alice","age":"30"}"#;
    let r = XmlWriteFn.execute(vec![Bytes::from(&json[..])], &p(&[("root_element","person")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<person>"));
    assert!(text.contains("<name>Alice</name>"));
    assert!(text.contains("</person>"));
}
#[test]
fn xml_write_nested() {
    let json = br#"{"db":{"host":"localhost","port":"5432"}}"#;
    let r = XmlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<host>localhost</host>"));
}
#[test]
fn xml_write_with_attributes() {
    let json = b"{\"@id\":\"42\",\"#text\":\"hello\"}";
    let r = XmlWriteFn.execute(vec![Bytes::from(&json[..])], &p(&[("root_element","item")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("id=\"42\""));
    assert!(text.contains("hello"));
}
#[test]
fn xml_write_default_root() {
    let json = br#"{"x":"1"}"#;
    let r = XmlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("<root>"));
}
#[test]
fn xml_write_includes_declaration() {
    let json = br#"{"a":"b"}"#;
    let r = XmlWriteFn.execute(vec![Bytes::from(&json[..])], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.starts_with("<?xml"));
}

// ---- XmlXPathExtractFn (#240) ----
#[test]
fn xpath_extract_child() {
    let xml = b"<root><name>Alice</name></root>";
    let r = XmlXPathExtractFn.execute(vec![Bytes::from(&xml[..])], &p(&[("xpath","/root/name")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("Alice"));
}
#[test]
fn xpath_extract_nested() {
    let xml = b"<config><db><host>localhost</host></db></config>";
    let r = XmlXPathExtractFn.execute(vec![Bytes::from(&xml[..])], &p(&[("xpath","/config/db/host")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("localhost"));
}
#[test]
fn xpath_extract_nonexistent_returns_null() {
    let xml = b"<root><a>1</a></root>";
    let r = XmlXPathExtractFn.execute(vec![Bytes::from(&xml[..])], &p(&[("xpath","/root/nonexistent")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("null"));
}
#[test]
fn xpath_extract_attribute() {
    let xml = b"<item id=\"42\"><name>Widget</name></item>";
    let r = XmlXPathExtractFn.execute(vec![Bytes::from(&xml[..])], &p(&[("xpath","/item")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("42"));
}
#[test]
fn xpath_extract_root_level() {
    let xml = b"<data><x>1</x></data>";
    let r = XmlXPathExtractFn.execute(vec![Bytes::from(&xml[..])], &p(&[("xpath","/data")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("x"));
}

// ---- XmlXsltTransformFn (#241) ----
#[test]
fn xslt_passthrough() {
    let xml = b"<root><a>1</a></root>";
    let xslt = b"<xsl:stylesheet/>";
    let r = XmlXsltTransformFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xslt[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn xslt_requires_two_inputs() {
    let r = XmlXsltTransformFn.execute(vec![Bytes::from("xml")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { .. })));
}
#[test]
fn xslt_valid_utf8_required() {
    let r = XmlXsltTransformFn.execute(vec![Bytes::from(vec![0xFF, 0xFE]), Bytes::from("xslt")], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn xslt_preserves_original_xml() {
    let xml = b"<?xml version=\"1.0\"?><doc><p>Hello</p></doc>";
    let xslt = b"<xsl:stylesheet/>";
    let r = XmlXsltTransformFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xslt[..])], &BTreeMap::new()).unwrap();
    assert!(std::str::from_utf8(&r).unwrap().contains("Hello"));
}
#[test]
fn xslt_empty_xml() {
    let xml = b"<root/>";
    let xslt = b"<xsl:stylesheet/>";
    let r = XmlXsltTransformFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xslt[..])], &BTreeMap::new()).unwrap();
    assert!(!r.is_empty());
}

// ---- XmlValidateDtdFn (#242) ----
#[test]
fn dtd_validate_wellformed() {
    let xml = b"<root><child>text</child></root>";
    let r = XmlValidateDtdFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn dtd_validate_malformed_error() {
    let xml = b"<root><a>1</b></root>"; // mismatched tags
    let r = XmlValidateDtdFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn dtd_validate_empty_element() {
    let xml = b"<root/>";
    let r = XmlValidateDtdFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn dtd_validate_with_attributes() {
    let xml = b"<item id=\"1\" name=\"test\"/>";
    let r = XmlValidateDtdFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn dtd_validate_nested_deep() {
    let xml = b"<a><b><c><d>deep</d></c></b></a>";
    let r = XmlValidateDtdFn.execute(vec![Bytes::from(&xml[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}

// ---- XmlValidateXsdFn (#243) ----
#[test]
fn xsd_validate_wellformed() {
    let xml = b"<root><child>text</child></root>";
    let xsd = b"<xs:schema/>";
    let r = XmlValidateXsdFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xsd[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn xsd_validate_requires_two_inputs() {
    let r = XmlValidateXsdFn.execute(vec![Bytes::from("xml")], &BTreeMap::new());
    assert!(matches!(r, Err(ComputeError::InputCount { .. })));
}
#[test]
fn xsd_validate_malformed_xml_error() {
    let xml = b"<root><a>1</b></root>"; // mismatched tags
    let xsd = b"<xs:schema/>";
    let r = XmlValidateXsdFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xsd[..])], &BTreeMap::new());
    assert!(r.is_err());
}
#[test]
fn xsd_validate_passthrough_on_success() {
    let xml = b"<data><item>1</item></data>";
    let xsd = b"<xs:schema/>";
    let r = XmlValidateXsdFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xsd[..])], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from(&xml[..]));
}
#[test]
fn xsd_validate_complex_xml() {
    let xml = b"<catalog><book id=\"1\"><title>Rust</title><price>29.99</price></book></catalog>";
    let xsd = b"<xs:schema/>";
    let r = XmlValidateXsdFn.execute(vec![Bytes::from(&xml[..]), Bytes::from(&xsd[..])], &BTreeMap::new()).unwrap();
    assert!(std::str::from_utf8(&r).unwrap().contains("Rust"));
}
