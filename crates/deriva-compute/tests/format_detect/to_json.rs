use bytes::Bytes;
use deriva_compute::function::ComputeFunction;
use deriva_compute::builtins_format_detect::UniversalToJsonFn;
use deriva_core::address::Value;
use std::collections::BTreeMap;

#[test]
fn csv_to_json_preserves_all_rows_and_columns() {
    let csv = b"product,price,qty\nWidget,9.99,100\nGadget,24.50,50\nDoohickey,4.75,200\n";
    let result = UniversalToJsonFn.execute(vec![Bytes::from(&csv[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&result).unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["product"], "Widget");
    assert_eq!(arr[0]["price"], "9.99");
    assert_eq!(arr[2]["qty"], "200");
}

#[test]
fn ndjson_to_json_array() {
    let ndjson = b"{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n";
    let result = UniversalToJsonFn.execute(vec![Bytes::from(&ndjson[..])], &BTreeMap::new()).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&result).unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[1]["a"], 2);
}

#[test]
fn yaml_to_json_preserves_structure() {
    let yaml = b"database:\n  host: localhost\n  port: 5432\nfeatures:\n  - auth\n  - logging\n";
    let result = UniversalToJsonFn.execute(vec![Bytes::from(&yaml[..])], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(v["database"]["host"], "localhost");
    assert_eq!(v["database"]["port"], 5432);
    assert_eq!(v["features"][0], "auth");
}

#[test]
fn max_records_limits_csv_output() {
    let mut csv = String::from("id,val\n");
    for i in 0..100 {
        csv.push_str(&format!("{},{}\n", i, i * 10));
    }
    let mut params = BTreeMap::new();
    params.insert("max_records".into(), Value::String("5".into()));
    let result = UniversalToJsonFn.execute(vec![Bytes::from(csv)], &params).unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&result).unwrap();
    assert_eq!(arr.len(), 5);
}

#[test]
fn binary_input_returns_summary_json() {
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let result = UniversalToJsonFn.execute(vec![Bytes::from(png)], &BTreeMap::new()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(v["format"], "png");
    assert!(v["summary"].as_str().unwrap().contains("bytes"));
}
