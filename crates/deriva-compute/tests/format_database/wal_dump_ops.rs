use bytes::Bytes;
use deriva_compute::builtins_format_database::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- wal_parse (5 tests) ----
#[test]
fn wal_parse_postgres() {
    let wal = "INSERT INTO users VALUES (1, 'Alice');\nUPDATE users SET age=30 WHERE id=1;\n";
    let out = WalParseFn.execute(vec![Bytes::from(wal)], &p(&[("format", "postgres")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"operation\":\"insert\""));
    assert!(text.contains("\"operation\":\"update\""));
}

#[test]
fn wal_parse_mysql() {
    let wal = "DELETE FROM users WHERE id=1;\n";
    let out = WalParseFn.execute(vec![Bytes::from(wal)], &p(&[("format", "mysql")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"operation\":\"delete\""));
}

#[test]
fn wal_parse_invalid_format() {
    assert!(WalParseFn.execute(vec![Bytes::from("x")], &p(&[("format", "oracle")])).is_err());
}

#[test]
fn wal_parse_no_input() {
    assert!(WalParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn wal_parse_id() {
    assert_eq!(WalParseFn.id().name, "wal_parse");
}

// ---- sql_dump_parse (5 tests) ----
#[test]
fn sql_dump_parse_create_table() {
    let dump = "CREATE TABLE users (id INT, name TEXT);\n";
    let out = SqlDumpParseFn.execute(vec![Bytes::from(dump)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"type\":\"create_table\""));
}

#[test]
fn sql_dump_parse_insert() {
    let dump = "INSERT INTO users VALUES (1, 'Alice');\nINSERT INTO users VALUES (2, 'Bob');\n";
    let out = SqlDumpParseFn.execute(vec![Bytes::from(dump)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(text.contains("\"type\":\"insert\""));
}

#[test]
fn sql_dump_parse_comments() {
    let dump = "-- Comment\nCREATE TABLE t (id INT);\n-- Another comment\n";
    let out = SqlDumpParseFn.execute(vec![Bytes::from(dump)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn sql_dump_parse_no_input() {
    assert!(SqlDumpParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn sql_dump_parse_id() {
    assert_eq!(SqlDumpParseFn.id().name, "sql_dump_parse");
}
