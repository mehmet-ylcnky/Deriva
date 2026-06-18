//! Category A: Columnar Data Formats — Parquet, ORC, Arrow (18 functions, #201–#218)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn fid(name: &str) -> FunctionId {
    FunctionId { name: name.to_string(), version: "1.0.0".to_string() }
}
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
    Ok(&inputs[0])
}
fn cost(sizes: &[u64]) -> ComputeCost {
    ComputeCost { cpu_ms: sizes.iter().sum::<u64>() / 100 + 50, memory_bytes: sizes.iter().sum::<u64>() * 2 + 1024 }
}
fn fail(msg: impl std::fmt::Display) -> ComputeError {
    ComputeError::ExecutionFailed(msg.to_string())
}

fn parse_compression(s: Option<&str>) -> Result<parquet::basic::Compression, ComputeError> {
    Ok(match s {
        Some("snappy") => parquet::basic::Compression::SNAPPY,
        Some("gzip") => parquet::basic::Compression::GZIP(parquet::basic::GzipLevel::default()),
        Some("lz4") => parquet::basic::Compression::LZ4,
        Some("zstd") => parquet::basic::Compression::ZSTD(parquet::basic::ZstdLevel::default()),
        Some("none") | None => parquet::basic::Compression::UNCOMPRESSED,
        Some(other) => return Err(ComputeError::InvalidParam(format!("unknown compression: {other}"))),
    })
}

fn read_parquet(data: &Bytes) -> Result<(SchemaRef, Vec<RecordBatch>), ComputeError> {
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(data.clone())
        .map_err(|e| fail(format!("parquet: {e}")))?;
    let schema = builder.schema().clone();
    let reader = builder.build().map_err(|e| fail(format!("parquet: {e}")))?;
    let batches: Result<Vec<_>, _> = reader.collect();
    Ok((schema, batches.map_err(|e| fail(format!("parquet: {e}")))?))
}

fn write_parquet(schema: &SchemaRef, batches: &[RecordBatch], comp: parquet::basic::Compression) -> Result<Bytes, ComputeError> {
    let props = parquet::file::properties::WriterProperties::builder().set_compression(comp).build();
    let mut buf = Vec::new();
    let mut w = parquet::arrow::ArrowWriter::try_new(&mut buf, schema.clone(), Some(props))
        .map_err(|e| fail(format!("parquet: {e}")))?;
    for b in batches { w.write(b).map_err(|e| fail(format!("parquet: {e}")))?; }
    w.close().map_err(|e| fail(format!("parquet: {e}")))?;
    Ok(Bytes::from(buf))
}

fn write_ipc(schema: &SchemaRef, batches: &[RecordBatch]) -> Result<Bytes, ComputeError> {
    let mut buf = Vec::new();
    let mut w = arrow::ipc::writer::StreamWriter::try_new(&mut buf, schema)
        .map_err(|e| fail(format!("ipc: {e}")))?;
    for b in batches { w.write(b).map_err(|e| fail(format!("ipc: {e}")))?; }
    w.finish().map_err(|e| fail(format!("ipc: {e}")))?;
    Ok(Bytes::from(buf))
}

fn read_ipc(data: &Bytes) -> Result<(SchemaRef, Vec<RecordBatch>), ComputeError> {
    let reader = arrow::ipc::reader::StreamReader::try_new(std::io::Cursor::new(data.as_ref()), None)
        .map_err(|e| fail(format!("ipc: {e}")))?;
    let schema = reader.schema();
    let batches: Result<Vec<_>, _> = reader.collect();
    Ok((schema, batches.map_err(|e| fail(format!("ipc: {e}")))?))
}

// ---- #201 ParquetReadFn ----
pub struct ParquetReadFn;
impl ComputeFunction for ParquetReadFn {
    fn id(&self) -> FunctionId { fid("parquet_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let (s, b) = read_parquet(one(&inputs)?)?;
        write_ipc(&s, &b)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #202 ParquetWriteFn ----
pub struct ParquetWriteFn;
impl ComputeFunction for ParquetWriteFn {
    fn id(&self) -> FunctionId { fid("parquet_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let comp = parse_compression(param_str(p, "compression").as_deref())?;
        let (s, b) = read_ipc(one(&inputs)?)?;
        write_parquet(&s, &b, comp)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #203 ParquetMetadataFn ----
pub struct ParquetMetadataFn;
impl ComputeFunction for ParquetMetadataFn {
    fn id(&self) -> FunctionId { fid("parquet_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(b.clone())
            .map_err(|e| fail(format!("parquet: {e}")))?;
        let meta = builder.metadata();
        let fm = meta.file_metadata();
        let rgs: Vec<_> = meta.row_groups().iter().map(|rg| serde_json::json!({
            "num_rows": rg.num_rows(),
            "total_byte_size": rg.total_byte_size(),
            "num_columns": rg.num_columns(),
        })).collect();
        let result = serde_json::json!({
            "version": fm.version(),
            "num_rows": fm.num_rows(),
            "created_by": fm.created_by(),
            "num_row_groups": meta.num_row_groups(),
            "row_groups": rgs,
        });
        Ok(Bytes::from(serde_json::to_string(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #204 ParquetProjectionFn ----
pub struct ParquetProjectionFn;
impl ComputeFunction for ParquetProjectionFn {
    fn id(&self) -> FunctionId { fid("parquet_projection") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let cols_str = param_str(p, "columns").ok_or_else(|| ComputeError::InvalidParam("columns required".into()))?;
        let cols: Vec<&str> = cols_str.split(',').map(|s| s.trim()).collect();
        let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(b.clone())
            .map_err(|e| fail(format!("parquet: {e}")))?;
        // Validate requested columns against schema (Requirement 9.3)
        let available: Vec<String> = builder.schema().fields().iter().map(|f| f.name().clone()).collect();
        let invalid: Vec<&str> = cols.iter().filter(|c| !available.iter().any(|a| a == *c)).copied().collect();
        if !invalid.is_empty() {
            return Err(ComputeError::InvalidParam(format!(
                "column(s) not found: {}. Available columns: {}",
                invalid.join(", "),
                available.join(", ")
            )));
        }
        let mask = parquet::arrow::ProjectionMask::columns(builder.parquet_schema(), cols.iter().copied());
        let schema = builder.schema().clone();
        let reader = builder.with_projection(mask).build().map_err(|e| fail(format!("parquet: {e}")))?;
        let batches: Result<Vec<_>, _> = reader.collect();
        let batches = batches.map_err(|e| fail(format!("parquet: {e}")))?;
        if batches.first().is_none_or(|b| b.num_columns() == 0) && !batches.is_empty() {
            return Err(ComputeError::InvalidParam("no matching columns found".into()));
        }
        let out_schema = if let Some(b) = batches.first() { b.schema() } else { schema };
        write_parquet(&out_schema, &batches, parquet::basic::Compression::UNCOMPRESSED)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #205 ParquetFilterFn ----
pub struct ParquetFilterFn;
impl ComputeFunction for ParquetFilterFn {
    fn id(&self) -> FunctionId { fid("parquet_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let col_name = param_str(p, "column").ok_or_else(|| ComputeError::InvalidParam("column required".into()))?;
        let op = param_str(p, "op").ok_or_else(|| ComputeError::InvalidParam("op required".into()))?;
        let value = param_str(p, "value");
        let (schema, batches) = read_parquet(b)?;
        let col_idx = schema.index_of(&col_name).map_err(|_| ComputeError::InvalidParam(format!("column not found: {col_name}")))?;
        let mut filtered = Vec::new();
        for batch in &batches {
            let mask = apply_filter(batch.column(col_idx), &op, value.as_deref())?;
            let fb = arrow::compute::filter_record_batch(batch, &mask).map_err(|e| fail(format!("filter: {e}")))?;
            if fb.num_rows() > 0 { filtered.push(fb); }
        }
        write_parquet(&schema, &filtered, parquet::basic::Compression::UNCOMPRESSED)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn apply_filter(col: &arrow::array::ArrayRef, op: &str, value: Option<&str>) -> Result<arrow::array::BooleanArray, ComputeError> {
    use arrow::array::*;
    
    match op {
        "is_null" => return arrow::compute::is_null(col.as_ref()).map_err(|e| fail(format!("filter: {e}"))),
        "is_not_null" => return arrow::compute::is_not_null(col.as_ref()).map_err(|e| fail(format!("filter: {e}"))),
        _ => {}
    }
    let val = value.ok_or_else(|| ComputeError::InvalidParam("value required for this op".into()))?;
    match col.data_type() {
        arrow::datatypes::DataType::Int32 => {
            let v: i32 = val.parse().map_err(|_| ComputeError::InvalidParam("invalid int32".into()))?;
            cmp_scalar(col, &Int32Array::new_scalar(v), op)
        }
        arrow::datatypes::DataType::Int64 => {
            let v: i64 = val.parse().map_err(|_| ComputeError::InvalidParam("invalid int64".into()))?;
            cmp_scalar(col, &Int64Array::new_scalar(v), op)
        }
        arrow::datatypes::DataType::Float64 => {
            let v: f64 = val.parse().map_err(|_| ComputeError::InvalidParam("invalid float64".into()))?;
            cmp_scalar(col, &Float64Array::new_scalar(v), op)
        }
        arrow::datatypes::DataType::Utf8 => cmp_scalar(col, &StringArray::new_scalar(val), op),
        arrow::datatypes::DataType::LargeUtf8 => cmp_scalar(col, &LargeStringArray::new_scalar(val), op),
        other => Err(fail(format!("unsupported filter type: {other:?}")))
    }
}

fn cmp_scalar(col: &arrow::array::ArrayRef, scalar: &dyn arrow::array::Datum, op: &str) -> Result<arrow::array::BooleanArray, ComputeError> {
    use arrow::compute::kernels::cmp;
    match op {
        "eq" => cmp::eq(col, scalar),
        "ne" => cmp::neq(col, scalar),
        "lt" => cmp::lt(col, scalar),
        "le" => cmp::lt_eq(col, scalar),
        "gt" => cmp::gt(col, scalar),
        "ge" => cmp::gt_eq(col, scalar),
        _ => return Err(ComputeError::InvalidParam(format!("unknown op: {op}"))),
    }.map_err(|e| fail(format!("filter: {e}")))
}

// ---- #206 ParquetMergeFn ----
pub struct ParquetMergeFn;
impl ComputeFunction for ParquetMergeFn {
    fn id(&self) -> FunctionId { fid("parquet_merge") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 1, got: 0 }); }
        let mut all = Vec::new();
        let mut schema: Option<SchemaRef> = None;
        for input in &inputs {
            let (s, batches) = read_parquet(input)?;
            if let Some(ref existing) = schema {
                if *existing != s { return Err(fail("merge: schemas must match")); }
            } else { schema = Some(s); }
            all.extend(batches);
        }
        write_parquet(&schema.unwrap(), &all, parquet::basic::Compression::UNCOMPRESSED)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #207 ParquetToArrowFn ----
pub struct ParquetToArrowFn;
impl ComputeFunction for ParquetToArrowFn {
    fn id(&self) -> FunctionId { fid("parquet_to_arrow") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let (s, b) = read_parquet(one(&inputs)?)?;
        write_ipc(&s, &b)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #208 ArrowToParquetFn ----
pub struct ArrowToParquetFn;
impl ComputeFunction for ArrowToParquetFn {
    fn id(&self) -> FunctionId { fid("arrow_to_parquet") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let comp = parse_compression(param_str(p, "compression").as_deref())?;
        let (s, b) = read_ipc(one(&inputs)?)?;
        write_parquet(&s, &b, comp)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #209 OrcReadFn ----
pub struct OrcReadFn;
impl ComputeFunction for OrcReadFn {
    fn id(&self) -> FunctionId { fid("orc_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?.clone();
        let builder = orc_rust::ArrowReaderBuilder::try_new(b).map_err(|e| fail(format!("orc: {e}")))?;
        let schema = builder.schema();
        let reader = builder.build();
        let batches: Result<Vec<_>, _> = reader.collect();
        write_ipc(&schema, &batches.map_err(|e| fail(format!("orc: {e}")))?)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #210 OrcWriteFn ----
pub struct OrcWriteFn;
impl ComputeFunction for OrcWriteFn {
    fn id(&self) -> FunctionId { fid("orc_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let (schema, batches) = read_ipc(one(&inputs)?)?;
        let mut buf = Vec::new();
        let mut w = orc_rust::ArrowWriterBuilder::new(&mut buf, schema)
            .try_build().map_err(|e| fail(format!("orc: {e}")))?;
        for b in &batches { w.write(b).map_err(|e| fail(format!("orc: {e}")))?; }
        w.close().map_err(|e| fail(format!("orc: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #211 OrcMetadataFn ----
pub struct OrcMetadataFn;
impl ComputeFunction for OrcMetadataFn {
    fn id(&self) -> FunctionId { fid("orc_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?.clone();
        let builder = orc_rust::ArrowReaderBuilder::try_new(b).map_err(|e| fail(format!("orc: {e}")))?;
        let meta = builder.file_metadata();
        let result = serde_json::json!({
            "num_rows": meta.number_of_rows(),
            "num_stripes": meta.stripe_metadatas().len(),
            "schema": format!("{}", builder.schema()),
        });
        Ok(Bytes::from(serde_json::to_string(&result).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #212 OrcFilterFn ----
pub struct OrcFilterFn;
impl ComputeFunction for OrcFilterFn {
    fn id(&self) -> FunctionId { fid("orc_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?.clone();
        let col_name = param_str(p, "column").ok_or_else(|| ComputeError::InvalidParam("column required".into()))?;
        let op = param_str(p, "op").ok_or_else(|| ComputeError::InvalidParam("op required".into()))?;
        let value = param_str(p, "value");
        let builder = orc_rust::ArrowReaderBuilder::try_new(b).map_err(|e| fail(format!("orc: {e}")))?;
        let schema = builder.schema();
        let col_idx = schema.index_of(&col_name).map_err(|_| ComputeError::InvalidParam(format!("column not found: {col_name}")))?;
        let reader = builder.build();
        let mut filtered = Vec::new();
        for batch in reader {
            let batch = batch.map_err(|e| fail(format!("orc: {e}")))?;
            let mask = apply_filter(batch.column(col_idx), &op, value.as_deref())?;
            let fb = arrow::compute::filter_record_batch(&batch, &mask).map_err(|e| fail(format!("filter: {e}")))?;
            if fb.num_rows() > 0 { filtered.push(fb); }
        }
        let write_schema = if let Some(b) = filtered.first() { b.schema() } else { schema };
        let mut buf = Vec::new();
        let mut w = orc_rust::ArrowWriterBuilder::new(&mut buf, write_schema)
            .try_build().map_err(|e| fail(format!("orc: {e}")))?;
        for b in &filtered { w.write(b).map_err(|e| fail(format!("orc: {e}")))?; }
        w.close().map_err(|e| fail(format!("orc: {e}")))?;
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #213 ArrowIpcReadFn ----
pub struct ArrowIpcReadFn;
impl ComputeFunction for ArrowIpcReadFn {
    fn id(&self) -> FunctionId { fid("arrow_ipc_read") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let (s, b) = read_ipc(one(&inputs)?)?;
        write_ipc(&s, &b)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #214 ArrowIpcWriteFn ----
pub struct ArrowIpcWriteFn;
impl ComputeFunction for ArrowIpcWriteFn {
    fn id(&self) -> FunctionId { fid("arrow_ipc_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let (s, b) = read_ipc(one(&inputs)?)?;
        write_ipc(&s, &b)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #215 ArrowSchemaExtractFn ----
pub struct ArrowSchemaExtractFn;
impl ComputeFunction for ArrowSchemaExtractFn {
    fn id(&self) -> FunctionId { fid("arrow_schema_extract") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let schema = if let Ok((s, _)) = read_ipc(b) { s }
        else {
            let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(b.clone())
                .map_err(|e| fail(format!("schema: {e}")))?;
            builder.schema().clone()
        };
        let fields: Vec<_> = schema.fields().iter().map(|f| serde_json::json!({
            "name": f.name(), "type": format!("{}", f.data_type()), "nullable": f.is_nullable(),
        })).collect();
        Ok(Bytes::from(serde_json::to_string(&serde_json::json!({"fields": fields, "metadata": schema.metadata()})).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #216 ParquetPartitionWriteFn ----
pub struct ParquetPartitionWriteFn;
impl ComputeFunction for ParquetPartitionWriteFn {
    fn id(&self) -> FunctionId { fid("parquet_partition_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        use arrow::array::*;
        let b = one(&inputs)?;
        let part_col = param_str(p, "partition_column").ok_or_else(|| ComputeError::InvalidParam("partition_column required".into()))?;
        let (schema, batches) = read_parquet(b)?;
        let col_idx = schema.index_of(&part_col).map_err(|_| ComputeError::InvalidParam(format!("column not found: {part_col}")))?;
        let mut partitions: BTreeMap<String, Vec<RecordBatch>> = BTreeMap::new();
        for batch in &batches {
            let col = batch.column(col_idx);
            let col_str = arrow::compute::cast(col, &arrow::datatypes::DataType::Utf8)
                .map_err(|e| fail(format!("cast: {e}")))?;
            let arr = col_str.as_any().downcast_ref::<StringArray>().ok_or_else(|| fail("cast failed"))?;
            let mut seen = Vec::new();
            for i in 0..arr.len() {
                if arr.is_valid(i) {
                    let v = arr.value(i).to_string();
                    if !seen.contains(&v) { seen.push(v); }
                }
            }
            for val in &seen {
                let mask: BooleanArray = (0..arr.len())
                    .map(|i| Some(arr.is_valid(i) && arr.value(i) == val.as_str()))
                    .collect();
                let fb = arrow::compute::filter_record_batch(batch, &mask).map_err(|e| fail(format!("filter: {e}")))?;
                if fb.num_rows() > 0 { partitions.entry(val.clone()).or_default().push(fb); }
            }
        }
        let mut manifest = serde_json::Map::new();
        for (val, pbs) in &partitions {
            let pq = write_parquet(&schema, pbs, parquet::basic::Compression::UNCOMPRESSED)?;
            manifest.insert(val.clone(), serde_json::json!({
                "partition_value": val,
                "num_rows": pbs.iter().map(|b| b.num_rows()).sum::<usize>(),
                "size_bytes": pq.len(),
            }));
        }
        Ok(Bytes::from(serde_json::to_string(&manifest).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #217 ParquetStatisticsFn ----
pub struct ParquetStatisticsFn;
impl ComputeFunction for ParquetStatisticsFn {
    fn id(&self) -> FunctionId { fid("parquet_statistics") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(b.clone())
            .map_err(|e| fail(format!("parquet: {e}")))?;
        let meta = builder.metadata();
        let pq_schema = builder.parquet_schema();
        let mut col_stats = serde_json::Map::new();
        for rg in meta.row_groups() {
            for i in 0..rg.num_columns() {
                let cc = rg.column(i);
                let name = pq_schema.column(i).name().to_string();
                if let Some(stats) = cc.statistics() {
                    col_stats.insert(name, serde_json::json!({
                        "min": stats.min_bytes_opt().map(|b| format!("{:?}", b)),
                        "max": stats.max_bytes_opt().map(|b| format!("{:?}", b)),
                        "null_count": stats.null_count_opt(),
                        "distinct_count": stats.distinct_count_opt(),
                        "num_values": cc.num_values(),
                    }));
                }
            }
        }
        Ok(Bytes::from(serde_json::to_string(&col_stats).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #218 ColumnarToRowFn ----
pub struct ColumnarToRowFn;
impl ComputeFunction for ColumnarToRowFn {
    fn id(&self) -> FunctionId { fid("columnar_to_row") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (_schema, batches) = if let Ok(r) = read_parquet(b) { r } else { read_ipc(b)? };
        let mut buf = Vec::new();
        {
            let mut w = arrow::json::LineDelimitedWriter::new(&mut buf);
            for batch in &batches { w.write(batch).map_err(|e| fail(format!("json: {e}")))?; }
            w.finish().map_err(|e| fail(format!("json: {e}")))?;
        }
        // Remove trailing newline if present
        if buf.last() == Some(&b'\n') { buf.pop(); }
        Ok(Bytes::from(buf))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
