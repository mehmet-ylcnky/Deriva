use bytes::Bytes;
use std::collections::BTreeMap;
use std::io::Cursor;
use deriva_core::address::{FunctionId, Value};
use crate::function::{ComputeCost, ComputeError, ComputeFunction};

fn fid(name: &str) -> FunctionId { FunctionId { name: name.into(), version: "1.0.0".into() } }
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    inputs.first().ok_or_else(|| ComputeError::InvalidParam("input required".into()))
}
fn fail(msg: String) -> ComputeError { ComputeError::ExecutionFailed(msg) }
fn cost(sizes: &[u64]) -> ComputeCost {
    let total: u64 = sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}

fn parse_page_ranges(s: &str, max_page: u32) -> Result<Vec<u32>, ComputeError> {
    let mut pages = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let start: u32 = a.trim().parse().map_err(|_| ComputeError::InvalidParam("invalid page range".into()))?;
            let end: u32 = b.trim().parse().map_err(|_| ComputeError::InvalidParam("invalid page range".into()))?;
            for p in start..=end.min(max_page) { pages.push(p); }
        } else {
            let p: u32 = part.parse().map_err(|_| ComputeError::InvalidParam("invalid page number".into()))?;
            if p <= max_page { pages.push(p); }
        }
    }
    Ok(pages)
}

// ---- #317 PdfMetadataFn ----
pub struct PdfMetadataFn;
impl ComputeFunction for PdfMetadataFn {
    fn id(&self) -> FunctionId { fid("pdf_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        let pages = doc.get_pages().len();
        let mut meta = serde_json::json!({ "pages": pages });
        if let Ok(info) = doc.trailer.get(b"Info").and_then(|o| doc.dereference(o)) {
            if let lopdf::Object::Dictionary(d) = info.1 {
                for (key, label) in [("Title", "title"), ("Author", "author"), ("Producer", "producer")] {
                    if let Ok(lopdf::Object::String(v, _)) = d.get(key.as_bytes()) {
                        meta[label] = serde_json::Value::String(String::from_utf8_lossy(v).into());
                    }
                }
            }
        }
        Ok(Bytes::from(serde_json::to_vec(&meta).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #318 PdfExtractTextFn ----
pub struct PdfExtractTextFn;
impl ComputeFunction for PdfExtractTextFn {
    fn id(&self) -> FunctionId { fid("pdf_extract_text") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        let max_page = doc.get_pages().len() as u32;
        let page_nums: Vec<u32> = if let Some(pages_str) = param_str(p, "pages") {
            parse_page_ranges(&pages_str, max_page)?
        } else {
            (1..=max_page).collect()
        };
        let text = doc.extract_text(&page_nums).map_err(|e| fail(format!("pdf text: {e}")))?;
        Ok(Bytes::from(text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #319 PdfExtractImagesFn ----
pub struct PdfExtractImagesFn;
impl ComputeFunction for PdfExtractImagesFn {
    fn id(&self) -> FunctionId { fid("pdf_extract_images") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        let mut images = Vec::new();
        for (i, page_id) in doc.page_iter().enumerate() {
            if let Ok((Some(resources), _)) = doc.get_page_resources(page_id) {
                if let Ok(xobjects) = resources.get(b"XObject") {
                    if let Ok(lopdf::Object::Dictionary(d)) = doc.dereference(xobjects).map(|(_, o)| o.clone()) {
                        for (name, _) in d.iter() {
                            images.push(serde_json::json!({
                                "page": i + 1,
                                "name": String::from_utf8_lossy(name),
                            }));
                        }
                    }
                }
            }
        }
        Ok(Bytes::from(serde_json::to_vec(&serde_json::json!({ "images": images })).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #320 PdfMergeFn ----
pub struct PdfMergeFn;
impl ComputeFunction for PdfMergeFn {
    fn id(&self) -> FunctionId { fid("pdf_merge") }
    #[allow(clippy::collapsible_if)]
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InvalidParam("at least one input required".into())); }
        if inputs.len() == 1 { return Ok(inputs[0].clone()); }
        let mut base = lopdf::Document::load_mem(&inputs[0]).map_err(|e| fail(format!("pdf: {e}")))?;
        let catalog = base.catalog().map_err(|e| fail(format!("pdf: {e}")))?;
        let pages_ref = match catalog.get(b"Pages").map_err(|e| fail(format!("pdf: {e}")))? {
            lopdf::Object::Reference(r) => *r,
            _ => return Err(fail("no pages ref".into())),
        };
        for inp in &inputs[1..] {
            let other = lopdf::Document::load_mem(inp).map_err(|e| fail(format!("pdf: {e}")))?;
            for page_id in other.page_iter() {
                if let Some(page_obj) = other.objects.get(&page_id) {
                    let mut page = page_obj.clone();
                    if let lopdf::Object::Dictionary(ref mut d) = page {
                        d.set("Parent", lopdf::Object::Reference(pages_ref));
                    }
                    let new_id = base.add_object(page);
                    if let Ok(lopdf::Object::Dictionary(ref mut d)) = base.get_object_mut(pages_ref) {
                        if let Ok(lopdf::Object::Array(ref mut arr)) = d.get_mut(b"Kids") {
                            arr.push(lopdf::Object::Reference(new_id));
                        }
                    }
                }
            }
        }
        // Update Count
        if let Ok(lopdf::Object::Dictionary(ref mut d)) = base.get_object_mut(pages_ref) {
            if let Ok(lopdf::Object::Array(arr)) = d.get(b"Kids") {
                d.set("Count", lopdf::Object::Integer(arr.len() as i64));
            }
        }
        let mut buf = Vec::new();
        base.save_to(&mut buf).map_err(|e| fail(format!("pdf save: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #321 PdfSplitFn ----
pub struct PdfSplitFn;
impl ComputeFunction for PdfSplitFn {
    fn id(&self) -> FunctionId { fid("pdf_split") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let pages_str = param_str(p, "pages").ok_or_else(|| ComputeError::InvalidParam("pages required".into()))?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        let max_page = doc.get_pages().len() as u32;
        let keep = parse_page_ranges(&pages_str, max_page)?;
        let mut new_doc = doc.clone();
        let all_pages: Vec<u32> = (1..=max_page).collect();
        let remove: Vec<u32> = all_pages.into_iter().filter(|p| !keep.contains(p)).collect();
        for page_num in remove.iter().rev() {
            new_doc.delete_pages(&[*page_num]);
        }
        let mut buf = Vec::new();
        new_doc.save_to(&mut buf).map_err(|e| fail(format!("pdf save: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #322 PdfPageCountFn ----
pub struct PdfPageCountFn;
impl ComputeFunction for PdfPageCountFn {
    fn id(&self) -> FunctionId { fid("pdf_page_count") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        Ok(Bytes::from(serde_json::to_vec(&serde_json::json!({ "pages": doc.get_pages().len() })).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #323 PdfToTextFn ----
pub struct PdfToTextFn;
impl ComputeFunction for PdfToTextFn {
    fn id(&self) -> FunctionId { fid("pdf_to_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let doc = lopdf::Document::load_mem(b).map_err(|e| fail(format!("pdf: {e}")))?;
        let pages: Vec<u32> = (1..=doc.get_pages().len() as u32).collect();
        let text = doc.extract_text(&pages).map_err(|e| fail(format!("pdf text: {e}")))?;
        Ok(Bytes::from(text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #324 DocxExtractTextFn ----
pub struct DocxExtractTextFn;
impl ComputeFunction for DocxExtractTextFn {
    fn id(&self) -> FunctionId { fid("docx_extract_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("docx zip: {e}")))?;
        let mut xml = String::new();
        {
            let mut f = archive.by_name("word/document.xml").map_err(|e| fail(format!("docx: {e}")))?;
            std::io::Read::read_to_string(&mut f, &mut xml).map_err(|e| fail(format!("docx read: {e}")))?;
        }
        let text = extract_xml_text(&xml);
        Ok(Bytes::from(text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #325 DocxMetadataFn ----
pub struct DocxMetadataFn;
impl ComputeFunction for DocxMetadataFn {
    fn id(&self) -> FunctionId { fid("docx_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("docx zip: {e}")))?;
        let mut meta = serde_json::json!({});
        if let Ok(mut f) = archive.by_name("docProps/core.xml") {
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut f, &mut xml).map_err(|e| fail(format!("read: {e}")))?;
            for (tag, key) in [("dc:title", "title"), ("dc:creator", "author"), ("cp:revision", "revision")] {
                if let Some(val) = extract_tag_content(&xml, tag) { meta[key] = serde_json::Value::String(val); }
            }
        }
        Ok(Bytes::from(serde_json::to_vec(&meta).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn extract_xml_text(xml: &str) -> String {
    // Simple: extract text between > and < that's inside <w:t> or similar text tags
    let mut result = String::new();
    #[allow(clippy::explicit_counter_loop)]
    let mut in_text = false;
    let mut depth = 0;
    for part in xml.split('<') {
        if depth > 0 {
            if let Some(close_pos) = part.find('>') {
                let tag = &part[..close_pos];
                let content = &part[close_pos + 1..];
                if tag.starts_with("w:t") && !tag.starts_with("w:t/") { in_text = true; }
                if tag == "/w:t" || tag.starts_with("/w:t ") { in_text = false; }
                if tag == "/w:p" || tag.starts_with("/w:p ") { result.push('\n'); }
                if in_text { result.push_str(content); }
            }
        }
        depth += 1;
    }
    result.trim().to_string()
}

fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = xml.find(&open) {
        let after_open = &xml[start..];
        if let Some(gt) = after_open.find('>') {
            let content_start = start + gt + 1;
            if let Some(end) = xml[content_start..].find(&close) {
                return Some(xml[content_start..content_start + end].to_string());
            }
        }
    }
    None
}

// ---- #326 XlsxReadFn ----
pub struct XlsxReadFn;
impl ComputeFunction for XlsxReadFn {
    fn id(&self) -> FunctionId { fid("xlsx_read") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        XlsxToCsvFn.execute(inputs, p)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #327 XlsxSheetListFn ----
pub struct XlsxSheetListFn;
impl ComputeFunction for XlsxSheetListFn {
    fn id(&self) -> FunctionId { fid("xlsx_sheet_list") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let wb = calamine::open_workbook_auto_from_rs(Cursor::new(b.as_ref()))
            .map_err(|e| fail(format!("xlsx: {e}")))?;
        use calamine::Reader;
        let names = wb.sheet_names().to_vec();
        Ok(Bytes::from(serde_json::to_vec(&names).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #328 XlsxToCsvFn ----
pub struct XlsxToCsvFn;
impl ComputeFunction for XlsxToCsvFn {
    fn id(&self) -> FunctionId { fid("xlsx_to_csv") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let sheet_param = param_str(p, "sheet").unwrap_or_else(|| "0".into());
        let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(b.as_ref()))
            .map_err(|e| fail(format!("xlsx: {e}")))?;
        use calamine::Reader;
        let sheet_name = if let Ok(idx) = sheet_param.parse::<usize>() {
            wb.sheet_names().get(idx)
                .ok_or_else(|| ComputeError::InvalidParam(format!("sheet index {idx} out of range")))?
                .clone()
        } else {
            sheet_param
        };
        let range = wb.worksheet_range(&sheet_name)
            .map_err(|e| fail(format!("sheet '{sheet_name}': {e}")))?;
        let mut wtr = csv::Writer::from_writer(Vec::new());
        for row in range.rows() {
            let record: Vec<String> = row.iter().map(|c| format!("{c}")).collect();
            wtr.write_record(&record).map_err(|e| fail(format!("csv: {e}")))?;
        }
        Ok(Bytes::from(wtr.into_inner().map_err(|e| fail(format!("csv: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #329 PptxExtractTextFn ----
pub struct PptxExtractTextFn;
impl ComputeFunction for PptxExtractTextFn {
    fn id(&self) -> FunctionId { fid("pptx_extract_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("pptx zip: {e}")))?;
        let mut all_text = String::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).map_err(|e| fail(format!("pptx: {e}")))?;
            if f.name().starts_with("ppt/slides/slide") && f.name().ends_with(".xml") {
                let mut xml = String::new();
                std::io::Read::read_to_string(&mut f, &mut xml).map_err(|e| fail(format!("read: {e}")))?;
                let text = extract_ooxml_text(&xml, "a:t");
                if !text.is_empty() {
                    if !all_text.is_empty() { all_text.push('\n'); }
                    all_text.push_str(&text);
                }
            }
        }
        Ok(Bytes::from(all_text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #330 PptxSlideCountFn ----
pub struct PptxSlideCountFn;
impl ComputeFunction for PptxSlideCountFn {
    fn id(&self) -> FunctionId { fid("pptx_slide_count") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("pptx zip: {e}")))?;
        let count = (0..archive.len()).filter(|&i| {
            archive.name_for_index(i).map(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).unwrap_or(false)
        }).count();
        Ok(Bytes::from(serde_json::to_vec(&serde_json::json!({ "slides": count })).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #331 HtmlToTextFn ----
pub struct HtmlToTextFn;
impl ComputeFunction for HtmlToTextFn {
    fn id(&self) -> FunctionId { fid("html_to_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let html = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let doc = scraper::Html::parse_document(html);
        let text: String = doc.root_element().text().collect::<Vec<_>>().join(" ");
        Ok(Bytes::from(text.split_whitespace().collect::<Vec<_>>().join(" ")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #332 HtmlExtractLinksFn ----
pub struct HtmlExtractLinksFn;
impl ComputeFunction for HtmlExtractLinksFn {
    fn id(&self) -> FunctionId { fid("html_extract_links") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let html = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let doc = scraper::Html::parse_document(html);
        let sel = scraper::Selector::parse("a[href]").unwrap();
        let links: Vec<&str> = doc.select(&sel).filter_map(|e| e.value().attr("href")).collect();
        Ok(Bytes::from(serde_json::to_vec(&links).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #333 HtmlExtractImagesFn ----
pub struct HtmlExtractImagesFn;
impl ComputeFunction for HtmlExtractImagesFn {
    fn id(&self) -> FunctionId { fid("html_extract_images") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let html = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let doc = scraper::Html::parse_document(html);
        let sel = scraper::Selector::parse("img[src]").unwrap();
        let srcs: Vec<&str> = doc.select(&sel).filter_map(|e| e.value().attr("src")).collect();
        Ok(Bytes::from(serde_json::to_vec(&srcs).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #334 HtmlMinifyFn ----
pub struct HtmlMinifyFn;
impl ComputeFunction for HtmlMinifyFn {
    fn id(&self) -> FunctionId { fid("html_minify") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let html = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        // Collapse whitespace runs, trim between tags
        let mut result = String::with_capacity(html.len());
        let mut in_tag = false;
        let mut last_was_space = false;
        for ch in html.chars() {
            if ch == '<' { in_tag = true; last_was_space = false; result.push(ch); }
            else if ch == '>' { in_tag = false; result.push(ch); }
            else if in_tag { result.push(ch); }
            else if ch.is_whitespace() {
                if !last_was_space { result.push(' '); last_was_space = true; }
            } else { result.push(ch); last_was_space = false; }
        }
        Ok(Bytes::from(result))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #335 MarkdownToHtmlFn ----
pub struct MarkdownToHtmlFn;
impl ComputeFunction for MarkdownToHtmlFn {
    fn id(&self) -> FunctionId { fid("markdown_to_html") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let md = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let ext_str = param_str(p, "extensions").unwrap_or_else(|| "all".into());
        let mut opts = comrak::Options::default();
        match ext_str.as_str() {
            "all" => { opts.extension.table = true; opts.extension.strikethrough = true; opts.extension.autolink = true; opts.extension.tasklist = true; }
            "tables" => { opts.extension.table = true; }
            "strikethrough" => { opts.extension.strikethrough = true; }
            _ => {}
        }
        let html = comrak::markdown_to_html(md, &opts);
        Ok(Bytes::from(html))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #336 HtmlToMarkdownFn ----
pub struct HtmlToMarkdownFn;
impl ComputeFunction for HtmlToMarkdownFn {
    fn id(&self) -> FunctionId { fid("html_to_markdown") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let html = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let doc = scraper::Html::parse_document(html);
        let md = html_node_to_md(&doc.root_element());
        Ok(Bytes::from(md.trim().to_string()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn html_node_to_md(el: &scraper::ElementRef) -> String {
    use scraper::Node;
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => {
                let child_ref = scraper::ElementRef::wrap(child);
                let inner = child_ref.map(|c| html_node_to_md(&c)).unwrap_or_default();
                match e.name() {
                    "h1" => { out.push_str(&format!("# {}\n\n", inner.trim())); }
                    "h2" => { out.push_str(&format!("## {}\n\n", inner.trim())); }
                    "h3" => { out.push_str(&format!("### {}\n\n", inner.trim())); }
                    "p" => { out.push_str(&format!("{}\n\n", inner.trim())); }
                    "strong" | "b" => { out.push_str(&format!("**{}**", inner)); }
                    "em" | "i" => { out.push_str(&format!("*{}*", inner)); }
                    "a" => {
                        let href = e.attr("href").unwrap_or("#");
                        out.push_str(&format!("[{}]({})", inner, href));
                    }
                    "br" => out.push('\n'),
                    "li" => { out.push_str(&format!("- {}\n", inner.trim())); }
                    "code" => { out.push_str(&format!("`{}`", inner)); }
                    _ => out.push_str(&inner),
                }
            }
            _ => {}
        }
    }
    out
}

// ---- #337 LatexToTextFn ----
pub struct LatexToTextFn;
impl ComputeFunction for LatexToTextFn {
    fn id(&self) -> FunctionId { fid("latex_to_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let latex = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = latex.chars().collect();
        while i < chars.len() {
            if chars[i] == '\\' {
                // Skip command name
                i += 1;
                while i < chars.len() && chars[i].is_alphabetic() { i += 1; }
                // Skip optional whitespace after command
                while i < chars.len() && chars[i] == ' ' { i += 1; }
                // If followed by {, skip the braces but keep content
                if i < chars.len() && chars[i] == '{' { i += 1; } // skip opening brace, content will be processed
            } else if chars[i] == '{' || chars[i] == '}' {
                i += 1;
            } else if chars[i] == '%' {
                // Skip comment to end of line
                while i < chars.len() && chars[i] != '\n' { i += 1; }
            } else if chars[i] == '$' {
                i += 1; // skip math delimiters
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        // Collapse whitespace
        let text = result.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok(Bytes::from(text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #338 EpubExtractTextFn ----
pub struct EpubExtractTextFn;
impl ComputeFunction for EpubExtractTextFn {
    fn id(&self) -> FunctionId { fid("epub_extract_text") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("epub zip: {e}")))?;
        let mut all_text = String::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).map_err(|e| fail(format!("epub: {e}")))?;
            let name = f.name().to_string();
            if name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm") {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut f, &mut content).map_err(|e| fail(format!("read: {e}")))?;
                let doc = scraper::Html::parse_document(&content);
                let text: String = doc.root_element().text().collect::<Vec<_>>().join(" ");
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    if !all_text.is_empty() { all_text.push('\n'); }
                    all_text.push_str(&text);
                }
            }
        }
        Ok(Bytes::from(all_text))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #339 EpubMetadataFn ----
pub struct EpubMetadataFn;
impl ComputeFunction for EpubMetadataFn {
    fn id(&self) -> FunctionId { fid("epub_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(b.as_ref())).map_err(|e| fail(format!("epub zip: {e}")))?;
        let mut meta = serde_json::json!({});
        // Find and parse content.opf
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).map_err(|e| fail(format!("epub: {e}")))?;
            let name = f.name().to_string();
            if name.ends_with(".opf") {
                let mut xml = String::new();
                std::io::Read::read_to_string(&mut f, &mut xml).map_err(|e| fail(format!("read: {e}")))?;
                for (tag, key) in [("dc:title", "title"), ("dc:creator", "author"), ("dc:language", "language")] {
                    if let Some(val) = extract_tag_content(&xml, tag) { meta[key] = serde_json::Value::String(val); }
                }
                break;
            }
        }
        // Count chapters (xhtml/html files)
        let chapters = (0..archive.len()).filter(|&i| {
            archive.name_for_index(i).map(|n| {
                (n.ends_with(".xhtml") || n.ends_with(".html")) && !n.contains("toc")
            }).unwrap_or(false)
        }).count();
        meta["chapters"] = serde_json::json!(chapters);
        Ok(Bytes::from(serde_json::to_vec(&meta).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn extract_ooxml_text(xml: &str, tag: &str) -> String {
    let mut result = String::new();
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;
    while let Some(start) = xml[pos..].find(&open) {
        let abs_start = pos + start;
        if let Some(gt) = xml[abs_start..].find('>') {
            let content_start = abs_start + gt + 1;
            if let Some(end) = xml[content_start..].find(&close) {
                let text = &xml[content_start..content_start + end];
                result.push_str(text);
                pos = content_start + end + close.len();
                continue;
            }
        }
        pos = abs_start + 1;
    }
    result
}
