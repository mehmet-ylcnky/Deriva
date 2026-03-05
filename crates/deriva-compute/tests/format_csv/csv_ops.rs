use bytes::Bytes;
use deriva_compute::function::{ComputeFunction, ComputeError};
use deriva_compute::builtins_format_csv::*;
use deriva_core::address::Value;
use std::collections::BTreeMap;

fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

const SALES_CSV: &[u8] = b"product,price,qty,region\nWidget,9.99,100,US\nGadget,24.50,50,EU\nDoohickey,4.75,200,US\nGizmo,15.00,75,EU\nThingamajig,8.25,150,US\n";

// ---- CsvParseFn (#219) ----
#[test]
fn csv_parse_realistic_sales_data() {
    let r = CsvParseFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("delimiter", ","), ("has_header", "true")])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 5);
    assert_eq!(arr[0]["product"], "Widget");
    assert_eq!(arr[4]["region"], "US");
}
#[test]
fn csv_parse_semicolon_delimiter() {
    let data = b"name;age;city\nAlice;30;NYC\nBob;25;SF\n";
    let r = CsvParseFn.execute(vec![Bytes::from(&data[..])], &p(&[("delimiter", ";")])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["name"], "Alice");
    assert_eq!(arr[1]["city"], "SF");
}
#[test]
fn csv_parse_no_header_generates_col_names() {
    let data = b"Alice,30,NYC\nBob,25,SF\n";
    let r = CsvParseFn.execute(vec![Bytes::from(&data[..])], &p(&[("has_header", "false")])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert!(arr[0].get("col0").is_some() || arr[0].get("col1").is_some());
}
#[test]
fn csv_parse_quoted_fields_with_commas() {
    let data = b"name,address\n\"Smith, John\",\"123 Main St, Apt 4\"\n";
    let r = CsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["name"], "Smith, John");
    assert!(arr[0]["address"].as_str().unwrap().contains("Apt 4"));
}
#[test]
fn csv_parse_empty_fields() {
    let data = b"a,b,c\n1,,3\n,2,\n";
    let r = CsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["b"], "");
    assert_eq!(arr[1]["a"], "");
}

// ---- CsvWriteFn (#220) ----
#[test]
fn csv_write_roundtrip_preserves_data() {
    let json = r#"[{"name":"Alice","age":"30"},{"name":"Bob","age":"25"}]"#;
    let csv = CsvWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let parsed = CsvParseFn.execute(vec![csv], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&parsed).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "Alice");
}
#[test]
fn csv_write_tab_delimiter() {
    let json = r#"[{"x":"1","y":"2"}]"#;
    let tsv = CsvWriteFn.execute(vec![Bytes::from(json)], &p(&[("delimiter", "\t")])).unwrap();
    let text = std::str::from_utf8(&tsv).unwrap();
    assert!(text.contains('\t'));
}
#[test]
fn csv_write_empty_array() {
    let r = CsvWriteFn.execute(vec![Bytes::from("[]")], &BTreeMap::new()).unwrap();
    assert!(r.is_empty());
}
#[test]
fn csv_write_special_characters() {
    let json = r#"[{"msg":"hello, world","note":"line1\nline2"}]"#;
    let csv = CsvWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&csv).unwrap();
    assert!(text.contains("hello, world") || text.contains("\"hello, world\""));
}
#[test]
fn csv_write_numeric_values() {
    let json = r#"[{"id":1,"val":3.14}]"#;
    let csv = CsvWriteFn.execute(vec![Bytes::from(json)], &BTreeMap::new()).unwrap();
    let text = std::str::from_utf8(&csv).unwrap();
    assert!(text.contains("3.14"));
}

// ---- CsvSchemaInferFn (#221) ----
#[test]
fn schema_infer_mixed_types() {
    let data = b"name,age,score,active\nAlice,30,95.5,true\nBob,25,87.0,false\n";
    let r = CsvSchemaInferFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let s: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(s["properties"]["age"]["type"], "integer");
    assert_eq!(s["properties"]["score"]["type"], "float");
    assert_eq!(s["properties"]["active"]["type"], "boolean");
}
#[test]
fn schema_infer_all_strings() {
    let data = b"city,country\nNYC,US\nLondon,UK\n";
    let r = CsvSchemaInferFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let s: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(s["properties"]["city"]["type"], "string");
}
#[test]
fn schema_infer_sample_rows_limit() {
    let mut csv = String::from("val\n");
    for i in 0..200 { csv.push_str(&format!("{}\n", i)); }
    let r = CsvSchemaInferFn.execute(vec![Bytes::from(csv)], &p(&[("sample_rows", "10")])).unwrap();
    let s: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(s["properties"]["val"]["type"], "integer");
}
#[test]
fn schema_infer_empty_column() {
    let data = b"id,notes\n1,\n2,\n3,\n";
    let r = CsvSchemaInferFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let s: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(s["properties"]["notes"]["type"], "string");
}
#[test]
fn schema_infer_float_column() {
    let data = b"temp\n36.6\n37.2\n36.9\n";
    let r = CsvSchemaInferFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let s: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(s["properties"]["temp"]["type"], "float");
}

// ---- CsvColumnSelectFn (#222) ----
#[test]
fn column_select_subset() {
    let r = CsvColumnSelectFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("columns", "product,region")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("product"));
    assert!(text.contains("region"));
    assert!(!text.contains("price"));
}
#[test]
fn column_select_single_column() {
    let r = CsvColumnSelectFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("columns", "qty")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("qty"));
    assert!(!text.contains("product"));
}
#[test]
fn column_select_nonexistent_returns_error() {
    let r = CsvColumnSelectFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("columns", "nonexistent")]));
    assert!(r.is_err());
}
#[test]
fn column_select_preserves_row_count() {
    let r = CsvColumnSelectFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("columns", "product")])).unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
    assert_eq!(lines.len(), 6); // header + 5 rows
}
#[test]
fn column_select_reorder() {
    let r = CsvColumnSelectFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("columns", "region,product")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    let first_line = text.lines().next().unwrap();
    assert!(first_line.starts_with("region") || first_line.starts_with("product"));
}

// ---- CsvColumnRenameFn (#223) ----
#[test]
fn column_rename_single() {
    let r = CsvColumnRenameFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("mapping", r#"{"product":"item"}"#)])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("item"));
    assert!(!text.lines().next().unwrap().contains("product"));
}
#[test]
fn column_rename_multiple() {
    let r = CsvColumnRenameFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("mapping", r#"{"product":"item","qty":"quantity"}"#)])).unwrap();
    let header = std::str::from_utf8(&r).unwrap().lines().next().unwrap();
    assert!(header.contains("item"));
    assert!(header.contains("quantity"));
}
#[test]
fn column_rename_nonexistent_key_ignored() {
    let r = CsvColumnRenameFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("mapping", r#"{"nonexistent":"foo"}"#)])).unwrap();
    let header = std::str::from_utf8(&r).unwrap().lines().next().unwrap();
    assert!(header.contains("product")); // unchanged
}
#[test]
fn column_rename_preserves_data() {
    let r = CsvColumnRenameFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("mapping", r#"{"price":"cost"}"#)])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("9.99")); // data preserved
}
#[test]
fn column_rename_preserves_row_count() {
    let r = CsvColumnRenameFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("mapping", r#"{"product":"p"}"#)])).unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&r).unwrap().lines().collect();
    assert_eq!(lines.len(), 6);
}

// ---- CsvFilterFn (#224) ----
#[test]
fn filter_eq_region() {
    let r = CsvFilterFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","region"),("op","eq"),("value","US")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let count = rdr.records().count();
    assert_eq!(count, 3); // Widget, Doohickey, Thingamajig
}
#[test]
fn filter_gt_numeric() {
    let r = CsvFilterFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty"),("op","gt"),("value","100")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("Doohickey")); // qty=200
    assert!(text.contains("Thingamajig")); // qty=150
    assert!(!text.contains("Widget")); // qty=100, not > 100
}
#[test]
fn filter_contains() {
    let r = CsvFilterFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","product"),("op","contains"),("value","get")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("Widget"));
    assert!(text.contains("Gadget"));
}
#[test]
fn filter_no_matches_returns_header_only() {
    let r = CsvFilterFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","region"),("op","eq"),("value","JP")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 0);
    // But header should still be present
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("product"));
}
#[test]
fn filter_nonexistent_column_error() {
    let r = CsvFilterFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","foo"),("op","eq"),("value","x")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ---- CsvSortFn (#225) ----
#[test]
fn sort_by_price_asc() {
    let r = CsvSortFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","price"),("order","asc")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows[0].get(0).unwrap(), "Doohickey"); // 4.75
    assert_eq!(rows[4].get(0).unwrap(), "Gadget"); // 24.50
}
#[test]
fn sort_by_product_desc() {
    let r = CsvSortFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","product"),("order","desc")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let first = rdr.records().next().unwrap().unwrap();
    assert_eq!(first.get(0).unwrap(), "Widget");
}
#[test]
fn sort_preserves_all_rows() {
    let r = CsvSortFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 5);
}
#[test]
fn sort_default_order_is_asc() {
    let r = CsvSortFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","price")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    let p0: f64 = rows[0].get(1).unwrap().parse().unwrap();
    let p1: f64 = rows[1].get(1).unwrap().parse().unwrap();
    assert!(p0 <= p1);
}
#[test]
fn sort_nonexistent_column_error() {
    let r = CsvSortFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","foo")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ---- CsvAggregateFn (#226) ----
#[test]
fn aggregate_sum() {
    let r = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty"),("function","sum")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["result"], 575.0); // 100+50+200+75+150
}
#[test]
fn aggregate_avg() {
    let r = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty"),("function","avg")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["result"], 115.0); // 575/5
}
#[test]
fn aggregate_count() {
    let r = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty"),("function","count")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(v["result"], 5);
}
#[test]
fn aggregate_min_max() {
    let rmin = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","price"),("function","min")])).unwrap();
    let rmax = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","price"),("function","max")])).unwrap();
    let vmin: serde_json::Value = serde_json::from_slice(&rmin).unwrap();
    let vmax: serde_json::Value = serde_json::from_slice(&rmax).unwrap();
    assert_eq!(vmin["result"], 4.75);
    assert_eq!(vmax["result"], 24.5);
}
#[test]
fn aggregate_unknown_function_error() {
    let r = CsvAggregateFn.execute(vec![Bytes::from(SALES_CSV)], &p(&[("column","qty"),("function","median")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ---- CsvJoinFn (#227) ----
#[test]
fn join_inner() {
    let left = b"id,name\n1,Alice\n2,Bob\n3,Charlie\n";
    let right = b"id,score\n1,95\n2,87\n4,72\n";
    let r = CsvJoinFn.execute(vec![Bytes::from(&left[..]), Bytes::from(&right[..])], &p(&[("join_column","id"),("join_type","inner")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 2); // id 1 and 2
}
#[test]
fn join_left_preserves_unmatched() {
    let left = b"id,name\n1,Alice\n2,Bob\n3,Charlie\n";
    let right = b"id,score\n1,95\n";
    let r = CsvJoinFn.execute(vec![Bytes::from(&left[..]), Bytes::from(&right[..])], &p(&[("join_column","id"),("join_type","left")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 3); // all left rows
}
#[test]
fn join_full_includes_all() {
    let left = b"id,name\n1,Alice\n";
    let right = b"id,score\n2,87\n";
    let r = CsvJoinFn.execute(vec![Bytes::from(&left[..]), Bytes::from(&right[..])], &p(&[("join_column","id"),("join_type","full")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    let rows: Vec<csv::StringRecord> = rdr.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 2);
}
#[test]
fn join_default_is_inner() {
    let left = b"k,v\na,1\nb,2\n";
    let right = b"k,w\na,x\nc,z\n";
    let r = CsvJoinFn.execute(vec![Bytes::from(&left[..]), Bytes::from(&right[..])], &p(&[("join_column","k")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 1); // only 'a'
}
#[test]
fn join_missing_column_error() {
    let left = b"id,name\n1,Alice\n";
    let right = b"key,score\n1,95\n";
    let r = CsvJoinFn.execute(vec![Bytes::from(&left[..]), Bytes::from(&right[..])], &p(&[("join_column","id")]));
    assert!(matches!(r, Err(ComputeError::InvalidParam(_))));
}

// ---- CsvDeduplicateFn (#228) ----
#[test]
fn deduplicate_full_row() {
    let data = b"name,age\nAlice,30\nBob,25\nAlice,30\nBob,25\n";
    let r = CsvDeduplicateFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 2);
}
#[test]
fn deduplicate_by_key_column() {
    let data = b"name,age\nAlice,30\nAlice,31\nBob,25\n";
    let r = CsvDeduplicateFn.execute(vec![Bytes::from(&data[..])], &p(&[("columns","name")])).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 2); // Alice(first) + Bob
}
#[test]
fn deduplicate_no_duplicates() {
    let r = CsvDeduplicateFn.execute(vec![Bytes::from(SALES_CSV)], &BTreeMap::new()).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 5);
}
#[test]
fn deduplicate_all_same() {
    let data = b"x\n1\n1\n1\n1\n";
    let r = CsvDeduplicateFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let mut rdr = csv::Reader::from_reader(&r[..]);
    assert_eq!(rdr.records().count(), 1);
}
#[test]
fn deduplicate_preserves_first_occurrence() {
    let data = b"name,val\nAlice,1\nAlice,2\n";
    let r = CsvDeduplicateFn.execute(vec![Bytes::from(&data[..])], &p(&[("columns","name")])).unwrap();
    let text = std::str::from_utf8(&r).unwrap();
    assert!(text.contains("Alice,1"));
    assert!(!text.contains("Alice,2"));
}

// ---- TsvParseFn (#229) ----
#[test]
fn tsv_parse_basic() {
    let data = b"name\tage\nAlice\t30\nBob\t25\n";
    let r = TsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "Alice");
}
#[test]
fn tsv_parse_with_empty_fields() {
    let data = b"a\tb\tc\n1\t\t3\n";
    let r = TsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["b"], "");
}
#[test]
fn tsv_parse_no_header() {
    let data = b"Alice\t30\nBob\t25\n";
    let r = TsvParseFn.execute(vec![Bytes::from(&data[..])], &p(&[("has_header","false")])).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 2);
}
#[test]
fn tsv_parse_single_column() {
    let data = b"val\n100\n200\n300\n";
    let r = TsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr.len(), 3);
}
#[test]
fn tsv_parse_many_columns() {
    let data = b"a\tb\tc\td\te\n1\t2\t3\t4\t5\n";
    let r = TsvParseFn.execute(vec![Bytes::from(&data[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&r).unwrap();
    assert_eq!(arr[0]["e"], "5");
}
