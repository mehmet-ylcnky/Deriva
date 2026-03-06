use bytes::Bytes;
use deriva_compute::builtins_format_document::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- docx_extract_text (5 tests) ----
#[test]
fn docx_extract_text_basic() {
    let docx = make_docx("Hello from DOCX");
    let out = DocxExtractTextFn.execute(vec![docx], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Hello from DOCX"));
}

#[test]
fn docx_extract_text_invalid() {
    assert!(DocxExtractTextFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn docx_extract_text_no_input() {
    assert!(DocxExtractTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn docx_extract_text_utf8() {
    let docx = make_docx("Test");
    let out = DocxExtractTextFn.execute(vec![docx], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).is_ok());
}

#[test]
fn docx_extract_text_id() {
    assert_eq!(DocxExtractTextFn.id().name, "docx_extract_text");
}

// ---- docx_metadata (5 tests) ----
#[test]
fn docx_metadata_has_title() {
    let docx = make_docx("Content");
    let out = DocxMetadataFn.execute(vec![docx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["title"], "Test Doc");
}

#[test]
fn docx_metadata_has_author() {
    let docx = make_docx("Content");
    let out = DocxMetadataFn.execute(vec![docx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["author"], "Test Author");
}

#[test]
fn docx_metadata_invalid() {
    assert!(DocxMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn docx_metadata_no_input() {
    assert!(DocxMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn docx_metadata_id() {
    assert_eq!(DocxMetadataFn.id().name, "docx_metadata");
}

// ---- xlsx_sheet_list (5 tests) ----
#[test]
fn xlsx_sheet_list_basic() {
    let xlsx = make_xlsx();
    let out = XlsxSheetListFn.execute(vec![xlsx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().len() >= 1);
    assert!(v.as_array().unwrap().iter().any(|s| s == "Data"));
}

#[test]
fn xlsx_sheet_list_invalid() {
    assert!(XlsxSheetListFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn xlsx_sheet_list_no_input() {
    assert!(XlsxSheetListFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn xlsx_sheet_list_is_array() {
    let xlsx = make_xlsx();
    let out = XlsxSheetListFn.execute(vec![xlsx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.is_array());
}

#[test]
fn xlsx_sheet_list_id() {
    assert_eq!(XlsxSheetListFn.id().name, "xlsx_sheet_list");
}

// ---- xlsx_to_csv (5 tests) ----
#[test]
fn xlsx_to_csv_default_sheet() {
    let xlsx = make_xlsx();
    let out = XlsxToCsvFn.execute(vec![xlsx], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("Name"));
    assert!(text.contains("Alice"));
}

#[test]
fn xlsx_to_csv_by_name() {
    let xlsx = make_xlsx();
    let out = XlsxToCsvFn.execute(vec![xlsx], &p(&[("sheet", "Data")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn xlsx_to_csv_invalid_sheet() {
    let xlsx = make_xlsx();
    assert!(XlsxToCsvFn.execute(vec![xlsx], &p(&[("sheet", "NoSuchSheet")])).is_err());
}

#[test]
fn xlsx_to_csv_invalid() {
    assert!(XlsxToCsvFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn xlsx_to_csv_no_input() {
    assert!(XlsxToCsvFn.execute(vec![], &p(&[])).is_err());
}

// ---- xlsx_read (5 tests) ----
#[test]
fn xlsx_read_basic() {
    let xlsx = make_xlsx();
    let out = XlsxReadFn.execute(vec![xlsx], &p(&[])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn xlsx_read_has_data() {
    let xlsx = make_xlsx();
    let out = XlsxReadFn.execute(vec![xlsx], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("42"));
}

#[test]
fn xlsx_read_invalid() {
    assert!(XlsxReadFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn xlsx_read_no_input() {
    assert!(XlsxReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn xlsx_read_id() {
    assert_eq!(XlsxReadFn.id().name, "xlsx_read");
}

// ---- pptx_extract_text (5 tests) ----
#[test]
fn pptx_extract_text_basic() {
    let pptx = make_pptx(&["Slide one text"]);
    let out = PptxExtractTextFn.execute(vec![pptx], &p(&[])).unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("Slide one text"));
}

#[test]
fn pptx_extract_text_multiple_slides() {
    let pptx = make_pptx(&["First", "Second"]);
    let out = PptxExtractTextFn.execute(vec![pptx], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("First"));
    assert!(text.contains("Second"));
}

#[test]
fn pptx_extract_text_invalid() {
    assert!(PptxExtractTextFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pptx_extract_text_no_input() {
    assert!(PptxExtractTextFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pptx_extract_text_id() {
    assert_eq!(PptxExtractTextFn.id().name, "pptx_extract_text");
}

// ---- pptx_slide_count (5 tests) ----
#[test]
fn pptx_slide_count_one() {
    let pptx = make_pptx(&["Only slide"]);
    let out = PptxSlideCountFn.execute(vec![pptx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["slides"], 1);
}

#[test]
fn pptx_slide_count_three() {
    let pptx = make_pptx(&["A", "B", "C"]);
    let out = PptxSlideCountFn.execute(vec![pptx], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["slides"], 3);
}

#[test]
fn pptx_slide_count_invalid() {
    assert!(PptxSlideCountFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn pptx_slide_count_no_input() {
    assert!(PptxSlideCountFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn pptx_slide_count_id() {
    assert_eq!(PptxSlideCountFn.id().name, "pptx_slide_count");
}
