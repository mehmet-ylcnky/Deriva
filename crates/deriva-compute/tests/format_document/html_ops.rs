use bytes::Bytes;
use deriva_compute::builtins_format_document::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- html_to_text (5 tests) ----
#[test]
fn html_to_text_basic() {
    let html = Bytes::from("<html><body><p>Hello World</p></body></html>");
    let out = HtmlToTextFn.execute(vec![html], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Hello World"));
}

#[test]
fn html_to_text_strips_tags() {
    let html = Bytes::from("<p><b>Bold</b> and <i>italic</i></p>");
    let out = HtmlToTextFn.execute(vec![html], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("Bold"));
    assert!(!text.contains("<b>"));
}

#[test]
fn html_to_text_no_input() {
    assert!(HtmlToTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn html_to_text_empty() {
    let html = Bytes::from("<html><body></body></html>");
    let out = HtmlToTextFn.execute(vec![html], &p(&[])).unwrap();
    assert!(out.is_empty() || std::str::from_utf8(&out).unwrap().trim().is_empty());
}

#[test]
fn html_to_text_id() {
    assert_eq!(HtmlToTextFn.id().name, "html_to_text");
}

// ---- html_extract_links (5 tests) ----
#[test]
fn html_extract_links_basic() {
    let html = Bytes::from(r#"<a href="https://example.com">Link</a>"#);
    let out = HtmlExtractLinksFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().contains(&serde_json::json!("https://example.com")));
}

#[test]
fn html_extract_links_multiple() {
    let html = Bytes::from(r#"<a href="/a">A</a><a href="/b">B</a>"#);
    let out = HtmlExtractLinksFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn html_extract_links_none() {
    let html = Bytes::from("<p>No links</p>");
    let out = HtmlExtractLinksFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

#[test]
fn html_extract_links_no_input() {
    assert!(HtmlExtractLinksFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn html_extract_links_id() {
    assert_eq!(HtmlExtractLinksFn.id().name, "html_extract_links");
}

// ---- html_extract_images (5 tests) ----
#[test]
fn html_extract_images_basic() {
    let html = Bytes::from(r#"<img src="photo.jpg"/>"#);
    let out = HtmlExtractImagesFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().contains(&serde_json::json!("photo.jpg")));
}

#[test]
fn html_extract_images_none() {
    let html = Bytes::from("<p>No images</p>");
    let out = HtmlExtractImagesFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

#[test]
fn html_extract_images_no_input() {
    assert!(HtmlExtractImagesFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn html_extract_images_multiple() {
    let html = Bytes::from(r#"<img src="a.png"/><img src="b.png"/>"#);
    let out = HtmlExtractImagesFn.execute(vec![html], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn html_extract_images_id() {
    assert_eq!(HtmlExtractImagesFn.id().name, "html_extract_images");
}

// ---- html_minify (5 tests) ----
#[test]
fn html_minify_collapses_whitespace() {
    let html = Bytes::from("<p>  Hello   World  </p>");
    let out = HtmlMinifyFn.execute(vec![html], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(!text.contains("  ")); // no double spaces
}

#[test]
fn html_minify_preserves_tags() {
    let html = Bytes::from("<p>Hello</p>");
    let out = HtmlMinifyFn.execute(vec![html], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("<p>"));
}

#[test]
fn html_minify_no_input() {
    assert!(HtmlMinifyFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn html_minify_smaller_output() {
    let html = Bytes::from("<p>  lots   of    spaces  </p>");
    let out = HtmlMinifyFn.execute(vec![html.clone()], &p(&[])).unwrap();
    assert!(out.len() <= html.len());
}

#[test]
fn html_minify_id() {
    assert_eq!(HtmlMinifyFn.id().name, "html_minify");
}
