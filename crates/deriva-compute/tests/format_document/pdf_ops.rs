use bytes::Bytes;
use deriva_compute::builtins_format_document::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- pdf_metadata (5 tests) ----
#[test]
fn pdf_metadata_has_pages() {
    let pdf = make_pdf("Hello");
    let out = PdfMetadataFn.execute(vec![pdf], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pages"], 1);
}

#[test]
fn pdf_metadata_invalid() {
    assert!(PdfMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_metadata_no_input() {
    assert!(PdfMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_metadata_is_json() {
    let pdf = make_pdf("Test");
    let out = PdfMetadataFn.execute(vec![pdf], &p(&[])).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
}

#[test]
fn pdf_metadata_id() {
    assert_eq!(PdfMetadataFn.id().name, "pdf_metadata");
}

// ---- pdf_extract_text (5 tests) ----
#[test]
fn pdf_extract_text_basic() {
    let pdf = make_pdf("Hello World");
    let out = PdfExtractTextFn.execute(vec![pdf], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("Hello World"));
}

#[test]
fn pdf_extract_text_with_pages() {
    let pdf = make_pdf("Page One");
    let out = PdfExtractTextFn.execute(vec![pdf], &p(&[("pages", "1")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn pdf_extract_text_invalid() {
    assert!(PdfExtractTextFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_extract_text_no_input() {
    assert!(PdfExtractTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_extract_text_id() {
    assert_eq!(PdfExtractTextFn.id().name, "pdf_extract_text");
}

// ---- pdf_extract_images (5 tests) ----
#[test]
fn pdf_extract_images_empty_pdf() {
    let pdf = make_pdf("No images");
    let out = PdfExtractImagesFn.execute(vec![pdf], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["images"].is_array());
}

#[test]
fn pdf_extract_images_invalid() {
    assert!(PdfExtractImagesFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_extract_images_no_input() {
    assert!(PdfExtractImagesFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_extract_images_is_json() {
    let pdf = make_pdf("Test");
    let out = PdfExtractImagesFn.execute(vec![pdf], &p(&[])).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
}

#[test]
fn pdf_extract_images_id() {
    assert_eq!(PdfExtractImagesFn.id().name, "pdf_extract_images");
}

// ---- pdf_merge (5 tests) ----
#[test]
fn pdf_merge_single() {
    let pdf = make_pdf("One");
    let merged = PdfMergeFn.execute(vec![pdf.clone()], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &PdfPageCountFn.execute(vec![merged], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["pages"], 1);
}

#[test]
fn pdf_merge_two() {
    let p1 = make_pdf("First");
    let p2 = make_pdf("Second");
    let merged = PdfMergeFn.execute(vec![p1, p2], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &PdfPageCountFn.execute(vec![merged], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["pages"], 2);
}

#[test]
fn pdf_merge_no_input() {
    assert!(PdfMergeFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_merge_invalid() {
    // Two invalid inputs forces lopdf to parse the second
    assert!(PdfMergeFn.execute(vec![Bytes::from_static(b"bad"), Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_merge_produces_valid_pdf() {
    let p1 = make_pdf("A");
    let merged = PdfMergeFn.execute(vec![p1.clone(), p1], &p(&[])).unwrap();
    assert!(PdfMetadataFn.execute(vec![merged], &p(&[])).is_ok());
}

// ---- pdf_split (5 tests) ----
#[test]
fn pdf_split_single_page() {
    let pdf = make_pdf("Only page");
    let split = PdfSplitFn.execute(vec![pdf], &p(&[("pages", "1")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &PdfPageCountFn.execute(vec![split], &p(&[])).unwrap()
    ).unwrap();
    assert_eq!(v["pages"], 1);
}

#[test]
fn pdf_split_missing_pages() {
    let pdf = make_pdf("Test");
    assert!(PdfSplitFn.execute(vec![pdf], &p(&[])).is_err());
}

#[test]
fn pdf_split_invalid() {
    assert!(PdfSplitFn.execute(vec![Bytes::from_static(b"bad")], &p(&[("pages", "1")])).is_err());
}

#[test]
fn pdf_split_no_input() {
    assert!(PdfSplitFn.execute(vec![], &p(&[("pages", "1")])).is_err());
}

#[test]
fn pdf_split_id() {
    assert_eq!(PdfSplitFn.id().name, "pdf_split");
}

// ---- pdf_page_count (5 tests) ----
#[test]
fn pdf_page_count_one() {
    let pdf = make_pdf("Test");
    let out = PdfPageCountFn.execute(vec![pdf], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pages"], 1);
}

#[test]
fn pdf_page_count_invalid() {
    assert!(PdfPageCountFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_page_count_no_input() {
    assert!(PdfPageCountFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_page_count_is_json() {
    let pdf = make_pdf("Test");
    let out = PdfPageCountFn.execute(vec![pdf], &p(&[])).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
}

#[test]
fn pdf_page_count_id() {
    assert_eq!(PdfPageCountFn.id().name, "pdf_page_count");
}

// ---- pdf_to_text (5 tests) ----
#[test]
fn pdf_to_text_basic() {
    let pdf = make_pdf("Extract me");
    let out = PdfToTextFn.execute(vec![pdf], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Extract me"));
}

#[test]
fn pdf_to_text_invalid() {
    assert!(PdfToTextFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pdf_to_text_no_input() {
    assert!(PdfToTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pdf_to_text_utf8() {
    let pdf = make_pdf("Hello");
    let out = PdfToTextFn.execute(vec![pdf], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).is_ok());
}

#[test]
fn pdf_to_text_id() {
    assert_eq!(PdfToTextFn.id().name, "pdf_to_text");
}
