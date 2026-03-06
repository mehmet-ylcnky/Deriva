use bytes::Bytes;
use deriva_compute::builtins_format_document::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- markdown_to_html (5 tests) ----
#[test]
fn markdown_to_html_basic() {
    let md = Bytes::from("# Hello\n\nWorld");
    let out = MarkdownToHtmlFn.execute(vec![md], &p(&[])).unwrap();
    let html = std::str::from_utf8(&out).unwrap();
    assert!(html.contains("<h1>"));
    assert!(html.contains("Hello"));
}

#[test]
fn markdown_to_html_tables() {
    let md = Bytes::from("| A | B |\n|---|---|\n| 1 | 2 |");
    let out = MarkdownToHtmlFn.execute(vec![md], &p(&[("extensions", "tables")])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("<table>"));
}

#[test]
fn markdown_to_html_no_input() {
    assert!(MarkdownToHtmlFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn markdown_to_html_empty() {
    let out = MarkdownToHtmlFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert!(out.is_empty() || std::str::from_utf8(&out).unwrap().trim().is_empty());
}

#[test]
fn markdown_to_html_id() {
    assert_eq!(MarkdownToHtmlFn.id().name, "markdown_to_html");
}

// ---- html_to_markdown (5 tests) ----
#[test]
fn html_to_markdown_basic() {
    let html = Bytes::from("<h1>Title</h1><p>Text</p>");
    let out = HtmlToMarkdownFn.execute(vec![html], &p(&[])).unwrap();
    let md = std::str::from_utf8(&out).unwrap();
    assert!(md.contains("# Title"));
}

#[test]
fn html_to_markdown_bold() {
    let html = Bytes::from("<p><strong>Bold</strong></p>");
    let out = HtmlToMarkdownFn.execute(vec![html], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("**Bold**"));
}

#[test]
fn html_to_markdown_link() {
    let html = Bytes::from(r#"<a href="https://example.com">Link</a>"#);
    let out = HtmlToMarkdownFn.execute(vec![html], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("[Link](https://example.com)"));
}

#[test]
fn html_to_markdown_no_input() {
    assert!(HtmlToMarkdownFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn html_to_markdown_id() {
    assert_eq!(HtmlToMarkdownFn.id().name, "html_to_markdown");
}

// ---- latex_to_text (5 tests) ----
#[test]
fn latex_to_text_basic() {
    let latex = Bytes::from(r"\documentclass{article}\begin{document}Hello World\end{document}");
    let out = LatexToTextFn.execute(vec![latex], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Hello World"));
}

#[test]
fn latex_to_text_strips_commands() {
    let latex = Bytes::from(r"\textbf{Bold text}");
    let out = LatexToTextFn.execute(vec![latex], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("Bold text"));
    assert!(!text.contains("\\textbf"));
}

#[test]
fn latex_to_text_strips_comments() {
    let latex = Bytes::from("Hello % this is a comment\nWorld");
    let out = LatexToTextFn.execute(vec![latex], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
    assert!(!text.contains("comment"));
}

#[test]
fn latex_to_text_no_input() {
    assert!(LatexToTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn latex_to_text_id() {
    assert_eq!(LatexToTextFn.id().name, "latex_to_text");
}

// ---- epub_extract_text (5 tests) ----
#[test]
fn epub_extract_text_basic() {
    let epub = make_epub("Test Book", "Chapter content here");
    let out = EpubExtractTextFn.execute(vec![epub], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Chapter content here"));
}

#[test]
fn epub_extract_text_invalid() {
    assert!(EpubExtractTextFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn epub_extract_text_no_input() {
    assert!(EpubExtractTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn epub_extract_text_utf8() {
    let epub = make_epub("Book", "Text");
    let out = EpubExtractTextFn.execute(vec![epub], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).is_ok());
}

#[test]
fn epub_extract_text_id() {
    assert_eq!(EpubExtractTextFn.id().name, "epub_extract_text");
}

// ---- epub_metadata (5 tests) ----
#[test]
fn epub_metadata_has_title() {
    let epub = make_epub("My Book", "Content");
    let out = EpubMetadataFn.execute(vec![epub], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["title"], "My Book");
}

#[test]
fn epub_metadata_has_author() {
    let epub = make_epub("Book", "Content");
    let out = EpubMetadataFn.execute(vec![epub], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["author"], "Test Author");
}

#[test]
fn epub_metadata_has_language() {
    let epub = make_epub("Book", "Content");
    let out = EpubMetadataFn.execute(vec![epub], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["language"], "en");
}

#[test]
fn epub_metadata_invalid() {
    assert!(EpubMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn epub_metadata_no_input() {
    assert!(EpubMetadataFn.execute(vec![], &p(&[])).is_err());
}
