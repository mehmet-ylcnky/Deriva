use bytes::Bytes;
use deriva_compute::builtins_format_database::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- postgres_copy_parse (5 tests) ----
#[test]
fn postgres_copy_parse_text() {
    let copy = "1\tAlice\t30\n2\tBob\t25\n";
    let out = PostgresCopyParseFn.execute(vec![Bytes::from(copy)], &p(&[])).unwrap();
    let csv = std::str::from_utf8(&out).unwrap();
    assert!(csv.contains("Alice"));
    assert!(csv.contains("Bob"));
}

#[test]
fn postgres_copy_parse_null() {
    let copy = "1\tAlice\t\\N\n";
    let out = PostgresCopyParseFn.execute(vec![Bytes::from(copy)], &p(&[])).unwrap();
    let csv = std::str::from_utf8(&out).unwrap();
    // \N becomes empty field in CSV
    assert!(csv.contains("Alice"));
}

#[test]
fn postgres_copy_parse_empty() {
    let out = PostgresCopyParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn postgres_copy_parse_no_input() {
    assert!(PostgresCopyParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn postgres_copy_parse_id() {
    assert_eq!(PostgresCopyParseFn.id().name, "postgres_copy_parse");
}

// ---- postgres_copy_write (5 tests) ----
#[test]
fn postgres_copy_write_basic() {
    let csv = "1,Alice,30\n2,Bob,25\n";
    let out = PostgresCopyWriteFn.execute(vec![Bytes::from(csv)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("1\tAlice\t30"));
    assert!(text.contains("2\tBob\t25"));
}

#[test]
fn postgres_copy_write_null() {
    let csv = "1,Alice,\n";
    let out = PostgresCopyWriteFn.execute(vec![Bytes::from(csv)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\\N"));
}

#[test]
fn postgres_copy_write_roundtrip() {
    let original = "1\tAlice\t30\n2\tBob\t\\N\n";
    let csv = PostgresCopyParseFn.execute(vec![Bytes::from(original)], &p(&[])).unwrap();
    let copy = PostgresCopyWriteFn.execute(vec![csv], &p(&[])).unwrap();
    let text = std::str::from_utf8(&copy).unwrap();
    assert!(text.contains("Alice"));
    assert!(text.contains("\\N"));
}

#[test]
fn postgres_copy_write_no_input() {
    assert!(PostgresCopyWriteFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn postgres_copy_write_id() {
    assert_eq!(PostgresCopyWriteFn.id().name, "postgres_copy_write");
}
