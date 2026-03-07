use bytes::Bytes;
use std::collections::BTreeMap;
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
fn cost(input_sizes: &[u64]) -> ComputeCost {
    let total: u64 = input_sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}

// ---- #411 SqliteQueryFn (requires rusqlite) ----
pub struct SqliteQueryFn;
impl ComputeFunction for SqliteQueryFn {
    fn id(&self) -> FunctionId { fid("sqlite_query") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("sqlite_query requires format-database-sqlite feature (rusqlite)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #412 SqliteTableListFn (requires rusqlite) ----
pub struct SqliteTableListFn;
impl ComputeFunction for SqliteTableListFn {
    fn id(&self) -> FunctionId { fid("sqlite_table_list") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("sqlite_table_list requires format-database-sqlite feature (rusqlite)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #413 SqliteToCsvFn (requires rusqlite) ----
pub struct SqliteToCsvFn;
impl ComputeFunction for SqliteToCsvFn {
    fn id(&self) -> FunctionId { fid("sqlite_to_csv") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("sqlite_to_csv requires format-database-sqlite feature (rusqlite)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #414 CsvToSqliteFn (requires rusqlite) ----
pub struct CsvToSqliteFn;
impl ComputeFunction for CsvToSqliteFn {
    fn id(&self) -> FunctionId { fid("csv_to_sqlite") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("csv_to_sqlite requires format-database-sqlite feature (rusqlite)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #415 PostgresCopyParseFn ----
pub struct PostgresCopyParseFn;
impl ComputeFunction for PostgresCopyParseFn {
    fn id(&self) -> FunctionId { fid("postgres_copy_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let format = param_str(p, "format").unwrap_or_else(|| "text".into());
        if format != "text" { return Err(ComputeError::InvalidParam("only text format supported".into())); }
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut wtr = csv::Writer::from_writer(Vec::new());
        for line in text.lines() {
            if line.is_empty() { continue; }
            let fields: Vec<&str> = line.split('\t').map(|f| if f == "\\N" { "" } else { f }).collect();
            wtr.write_record(&fields).map_err(|e| fail(format!("csv: {e}")))?;
        }
        Ok(Bytes::from(wtr.into_inner().map_err(|e| fail(format!("csv: {e}")))?))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #416 PostgresCopyWriteFn ----
pub struct PostgresCopyWriteFn;
impl ComputeFunction for PostgresCopyWriteFn {
    fn id(&self) -> FunctionId { fid("postgres_copy_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let format = param_str(p, "format").unwrap_or_else(|| "text".into());
        if format != "text" { return Err(ComputeError::InvalidParam("only text format supported".into())); }
        let mut rdr = csv::ReaderBuilder::new().has_headers(false).from_reader(b.as_ref());
        let mut lines = Vec::new();
        for result in rdr.records() {
            let record = result.map_err(|e| fail(format!("csv: {e}")))?;
            let fields: Vec<&str> = record.iter().map(|f| if f.is_empty() { "\\N" } else { f }).collect();
            lines.push(fields.join("\t"));
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #417 SstMetadataFn (requires rocksdb) ----
pub struct SstMetadataFn;
impl ComputeFunction for SstMetadataFn {
    fn id(&self) -> FunctionId { fid("sst_metadata") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("sst_metadata requires format-database-rocksdb feature (rocksdb)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #418 SstScanFn (requires rocksdb) ----
pub struct SstScanFn;
impl ComputeFunction for SstScanFn {
    fn id(&self) -> FunctionId { fid("sst_scan") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("sst_scan requires format-database-rocksdb feature (rocksdb)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #419 WalParseFn ----
pub struct WalParseFn;
impl ComputeFunction for WalParseFn {
    fn id(&self) -> FunctionId { fid("wal_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let format = param_str(p, "format").unwrap_or_else(|| "postgres".into());
        if format != "postgres" && format != "mysql" {
            return Err(ComputeError::InvalidParam("format: postgres or mysql".into()));
        }
        // Simplified: treat as text log with operation markers
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let op_type = if line.contains("INSERT") { "insert" }
                else if line.contains("UPDATE") { "update" }
                else if line.contains("DELETE") { "delete" }
                else { "unknown" };
            let record = serde_json::json!({
                "operation": op_type,
                "format": format,
                "raw": line,
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #420 SqlDumpParseFn ----
pub struct SqlDumpParseFn;
impl ComputeFunction for SqlDumpParseFn {
    fn id(&self) -> FunctionId { fid("sql_dump_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        let mut stmt = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("--") { continue; }
            stmt.push_str(line);
            stmt.push(' ');
            if trimmed.ends_with(';') {
                let normalized = stmt.trim().to_uppercase();
                let stmt_type = if normalized.starts_with("CREATE TABLE") { "create_table" }
                    else if normalized.starts_with("INSERT") { "insert" }
                    else if normalized.starts_with("COPY") { "copy" }
                    else { "other" };
                let record = serde_json::json!({
                    "type": stmt_type,
                    "statement": stmt.trim(),
                });
                lines.push(serde_json::to_string(&record).unwrap());
                stmt.clear();
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
