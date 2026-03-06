use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;
use std::io::Write;
use lopdf::dictionary;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Create a minimal valid PDF with given text content
pub fn make_pdf(text: &str) -> Bytes {
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(lopdf::dictionary! {
        "Font" => lopdf::dictionary! { "F1" => font_id },
    });
    let content = lopdf::content::Content {
        operations: vec![
            lopdf::content::Operation::new("BT", vec![]),
            lopdf::content::Operation::new("Tf", vec!["F1".into(), 12.into()]),
            lopdf::content::Operation::new("Td", vec![100.into(), 700.into()]),
            lopdf::content::Operation::new("Tj", vec![lopdf::Object::string_literal(text)]),
            lopdf::content::Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(lopdf::Stream::new(lopdf::dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "Resources" => resources_id,
    });
    let pages = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, lopdf::Object::Dictionary(pages));
    let catalog_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut buf = Vec::new();
    doc.save_to(&mut buf).unwrap();
    Bytes::from(buf)
}

/// Create a minimal DOCX (ZIP with word/document.xml)
pub fn make_docx(text: &str) -> Bytes {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body>
</w:document>"#).unwrap();
        zip.start_file("docProps/core.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?>
<cp:coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">
<dc:title>Test Doc</dc:title><dc:creator>Test Author</dc:creator><cp:revision>3</cp:revision>
</cp:coreProperties>"#).unwrap();
        zip.start_file("[Content_Types].xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>"#).unwrap();
        zip.finish().unwrap();
    }
    Bytes::from(buf)
}

/// Create a minimal XLSX using calamine-compatible format (via zip + XML)
pub fn make_xlsx() -> Bytes {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("[Content_Types].xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#).unwrap();
        zip.start_file("_rels/.rels", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#).unwrap();
        zip.start_file("xl/_rels/workbook.xml.rels", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#).unwrap();
        zip.start_file("xl/workbook.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#).unwrap();
        zip.start_file("xl/sharedStrings.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
<si><t>Name</t></si><si><t>Alice</t></si></sst>"#).unwrap();
        zip.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42</v></c></row>
<row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>99</v></c></row>
</sheetData></worksheet>"#).unwrap();
        zip.finish().unwrap();
    }
    Bytes::from(buf)
}

/// Create a minimal PPTX
pub fn make_pptx(slide_texts: &[&str]) -> Bytes {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("[Content_Types].xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"></Types>"#).unwrap();
        for (i, text) in slide_texts.iter().enumerate() {
            zip.start_file(format!("ppt/slides/slide{}.xml", i + 1), opts).unwrap();
            write!(zip, r#"<?xml version="1.0"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#).unwrap();
        }
        zip.finish().unwrap();
    }
    Bytes::from(buf)
}

/// Create a minimal EPUB
pub fn make_epub(title: &str, text: &str) -> Bytes {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
        write!(zip, "application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
<rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("content.opf", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>Test Author</dc:creator><dc:language>en</dc:language></metadata>
<manifest><item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="ch1"/></spine></package>"#).unwrap();
        zip.start_file("chapter1.xhtml", opts).unwrap();
        write!(zip, r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>{text}</p></body></html>"#).unwrap();
        zip.finish().unwrap();
    }
    Bytes::from(buf)
}
