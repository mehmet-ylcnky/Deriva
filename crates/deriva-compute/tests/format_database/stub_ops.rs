use bytes::Bytes;
use deriva_compute::builtins_format_database::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- sqlite_query (5 tests) ----
#[test]
fn sqlite_query_requires_lib() { assert!(SqliteQueryFn.execute(vec![Bytes::new()], &p(&[("sql", "SELECT 1")])).is_err()); }
#[test]
fn sqlite_query_id() { assert_eq!(SqliteQueryFn.id().name, "sqlite_query"); }
#[test]
fn sqlite_query_no_input() { assert!(SqliteQueryFn.execute(vec![], &p(&[("sql", "SELECT 1")])).is_err()); }
#[test]
fn sqlite_query_with_sql() { assert!(SqliteQueryFn.execute(vec![Bytes::new()], &p(&[("sql", "SELECT * FROM t")])).is_err()); }
#[test]
fn sqlite_query_error_msg() {
    let err = SqliteQueryFn.execute(vec![Bytes::new()], &p(&[("sql", "SELECT 1")])).unwrap_err();
    assert!(format!("{err:?}").contains("rusqlite"));
}

// ---- sqlite_table_list (5 tests) ----
#[test]
fn sqlite_table_list_requires_lib() { assert!(SqliteTableListFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn sqlite_table_list_id() { assert_eq!(SqliteTableListFn.id().name, "sqlite_table_list"); }
#[test]
fn sqlite_table_list_no_input() { assert!(SqliteTableListFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn sqlite_table_list_with_data() { assert!(SqliteTableListFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn sqlite_table_list_error_msg() {
    let err = SqliteTableListFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-database-sqlite"));
}

// ---- sqlite_to_csv (5 tests) ----
#[test]
fn sqlite_to_csv_requires_lib() { assert!(SqliteToCsvFn.execute(vec![Bytes::new()], &p(&[("table", "users")])).is_err()); }
#[test]
fn sqlite_to_csv_id() { assert_eq!(SqliteToCsvFn.id().name, "sqlite_to_csv"); }
#[test]
fn sqlite_to_csv_no_input() { assert!(SqliteToCsvFn.execute(vec![], &p(&[("table", "t")])).is_err()); }
#[test]
fn sqlite_to_csv_with_table() { assert!(SqliteToCsvFn.execute(vec![Bytes::new()], &p(&[("table", "users")])).is_err()); }
#[test]
fn sqlite_to_csv_error_msg() {
    let err = SqliteToCsvFn.execute(vec![Bytes::new()], &p(&[("table", "t")])).unwrap_err();
    assert!(format!("{err:?}").contains("rusqlite"));
}

// ---- csv_to_sqlite (5 tests) ----
#[test]
fn csv_to_sqlite_requires_lib() { assert!(CsvToSqliteFn.execute(vec![Bytes::from("a,b\n1,2\n")], &p(&[("table", "t")])).is_err()); }
#[test]
fn csv_to_sqlite_id() { assert_eq!(CsvToSqliteFn.id().name, "csv_to_sqlite"); }
#[test]
fn csv_to_sqlite_no_input() { assert!(CsvToSqliteFn.execute(vec![], &p(&[("table", "t")])).is_err()); }
#[test]
fn csv_to_sqlite_with_csv() { assert!(CsvToSqliteFn.execute(vec![Bytes::from("a\n1\n")], &p(&[("table", "t")])).is_err()); }
#[test]
fn csv_to_sqlite_error_msg() {
    let err = CsvToSqliteFn.execute(vec![Bytes::from("a\n1\n")], &p(&[("table", "t")])).unwrap_err();
    assert!(format!("{err:?}").contains("format-database-sqlite"));
}

// ---- sst_metadata (5 tests) ----
#[test]
fn sst_metadata_requires_lib() { assert!(SstMetadataFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn sst_metadata_id() { assert_eq!(SstMetadataFn.id().name, "sst_metadata"); }
#[test]
fn sst_metadata_no_input() { assert!(SstMetadataFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn sst_metadata_with_data() { assert!(SstMetadataFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn sst_metadata_error_msg() {
    let err = SstMetadataFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("rocksdb"));
}

// ---- sst_scan (5 tests) ----
#[test]
fn sst_scan_requires_lib() { assert!(SstScanFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn sst_scan_id() { assert_eq!(SstScanFn.id().name, "sst_scan"); }
#[test]
fn sst_scan_no_input() { assert!(SstScanFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn sst_scan_with_range() { assert!(SstScanFn.execute(vec![Bytes::new()], &p(&[("start_key", "a"), ("end_key", "z")])).is_err()); }
#[test]
fn sst_scan_error_msg() {
    let err = SstScanFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-database-rocksdb"));
}
