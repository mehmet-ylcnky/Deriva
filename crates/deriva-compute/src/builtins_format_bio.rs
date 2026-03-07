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

// ---- #441 FastaParseFn ----
pub struct FastaParseFn;
impl ComputeFunction for FastaParseFn {
    fn id(&self) -> FunctionId { fid("fasta_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        let mut current_id = String::new();
        let mut current_seq = String::new();
        for line in text.lines() {
            if line.starts_with('>') {
                if !current_id.is_empty() {
                    let record = serde_json::json!({
                        "id": current_id,
                        "sequence": current_seq,
                        "length": current_seq.len(),
                    });
                    lines.push(serde_json::to_string(&record).unwrap());
                }
                current_id = line[1..].to_string();
                current_seq.clear();
            } else {
                current_seq.push_str(line.trim());
            }
        }
        if !current_id.is_empty() {
            let record = serde_json::json!({
                "id": current_id,
                "sequence": current_seq,
                "length": current_seq.len(),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #442 FastqParseFn ----
pub struct FastqParseFn;
impl ComputeFunction for FastqParseFn {
    fn id(&self) -> FunctionId { fid("fastq_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        let all_lines: Vec<&str> = text.lines().collect();
        for chunk in all_lines.chunks(4) {
            if chunk.len() < 4 { break; }
            if !chunk[0].starts_with('@') { continue; }
            let record = serde_json::json!({
                "id": &chunk[0][1..],
                "sequence": chunk[1],
                "quality": chunk[3],
                "length": chunk[1].len(),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #443 FastqTrimQualityFn ----
pub struct FastqTrimQualityFn;
impl ComputeFunction for FastqTrimQualityFn {
    fn id(&self) -> FunctionId { fid("fastq_trim_quality") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let min_quality = param_str(p, "min_quality").unwrap_or_else(|| "20".into()).parse::<u8>().unwrap_or(20);
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut out = Vec::new();
        let all_lines: Vec<&str> = text.lines().collect();
        for chunk in all_lines.chunks(4) {
            if chunk.len() < 4 { break; }
            let qual = chunk[3].as_bytes();
            let mut trim_len = qual.len();
            for i in (0..qual.len()).rev() {
                if qual[i] < 33 + min_quality { trim_len = i; } else { break; }
            }
            if trim_len > 0 {
                out.push(chunk[0].as_bytes());
                out.push(b"\n");
                out.push(&chunk[1].as_bytes()[..trim_len]);
                out.push(b"\n+\n");
                out.push(&qual[..trim_len]);
                out.push(b"\n");
            }
        }
        Ok(Bytes::from(out.concat()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #444 FastqFilterFn ----
pub struct FastqFilterFn;
impl ComputeFunction for FastqFilterFn {
    fn id(&self) -> FunctionId { fid("fastq_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let min_length = param_str(p, "min_length").and_then(|s| s.parse::<usize>().ok());
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut out = Vec::new();
        let all_lines: Vec<&str> = text.lines().collect();
        for chunk in all_lines.chunks(4) {
            if chunk.len() < 4 { break; }
            let seq_len = chunk[1].len();
            if let Some(min) = min_length {
                if seq_len < min { continue; }
            }
            out.push(chunk[0].as_bytes());
            out.push(b"\n");
            out.push(chunk[1].as_bytes());
            out.push(b"\n+\n");
            out.push(chunk[3].as_bytes());
            out.push(b"\n");
        }
        Ok(Bytes::from(out.concat()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #445 SamParseFn ----
pub struct SamParseFn;
impl ComputeFunction for SamParseFn {
    fn id(&self) -> FunctionId { fid("sam_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.starts_with('@') { continue; }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 11 { continue; }
            let record = serde_json::json!({
                "qname": parts[0],
                "flag": parts[1].parse::<u16>().ok(),
                "rname": parts[2],
                "pos": parts[3].parse::<u32>().ok(),
                "mapq": parts[4].parse::<u8>().ok(),
                "cigar": parts[5],
                "seq": parts[9],
                "qual": parts[10],
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #446 BamReadFn (requires noodles/bio) ----
pub struct BamReadFn;
impl ComputeFunction for BamReadFn {
    fn id(&self) -> FunctionId { fid("bam_read") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("bam_read requires format-bio-bam feature (noodles)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #447 BamToSamFn (requires noodles/bio) ----
pub struct BamToSamFn;
impl ComputeFunction for BamToSamFn {
    fn id(&self) -> FunctionId { fid("bam_to_sam") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("bam_to_sam requires format-bio-bam feature (noodles)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #448 VcfParseFn ----
pub struct VcfParseFn;
impl ComputeFunction for VcfParseFn {
    fn id(&self) -> FunctionId { fid("vcf_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') { continue; }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 8 { continue; }
            let record = serde_json::json!({
                "chrom": parts[0],
                "pos": parts[1].parse::<u32>().ok(),
                "id": parts[2],
                "ref": parts[3],
                "alt": parts[4].split(',').collect::<Vec<_>>(),
                "qual": parts[5].parse::<f32>().ok(),
                "filter": parts[6],
                "info": parts[7],
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #449 VcfFilterFn ----
pub struct VcfFilterFn;
impl ComputeFunction for VcfFilterFn {
    fn id(&self) -> FunctionId { fid("vcf_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let chrom_filter = param_str(p, "chrom");
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut out = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') {
                out.push(line.as_bytes());
                out.push(b"\n");
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() { continue; }
            if let Some(ref chrom) = chrom_filter {
                if parts[0] != chrom { continue; }
            }
            out.push(line.as_bytes());
            out.push(b"\n");
        }
        Ok(Bytes::from(out.concat()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #450 BedParseFn ----
pub struct BedParseFn;
impl ComputeFunction for BedParseFn {
    fn id(&self) -> FunctionId { fid("bed_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 { continue; }
            let record = serde_json::json!({
                "chrom": parts[0],
                "start": parts[1].parse::<u32>().ok(),
                "end": parts[2].parse::<u32>().ok(),
                "name": parts.get(3).unwrap_or(&"."),
                "score": parts.get(4).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
                "strand": parts.get(5).unwrap_or(&"."),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #451 GffParseFn ----
pub struct GffParseFn;
impl ComputeFunction for GffParseFn {
    fn id(&self) -> FunctionId { fid("gff_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() { continue; }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 9 { continue; }
            let record = serde_json::json!({
                "seqid": parts[0],
                "source": parts[1],
                "type": parts[2],
                "start": parts[3].parse::<u32>().ok(),
                "end": parts[4].parse::<u32>().ok(),
                "score": parts[5].parse::<f32>().ok(),
                "strand": parts[6],
                "phase": parts[7],
                "attributes": parts[8],
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #452 FastaToFastqFn ----
pub struct FastaToFastqFn;
impl ComputeFunction for FastaToFastqFn {
    fn id(&self) -> FunctionId { fid("fasta_to_fastq") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let default_quality = param_str(p, "default_quality").unwrap_or_else(|| "I".into());
        let qual_char = default_quality.chars().next().unwrap_or('I');
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut out = Vec::new();
        let mut current_id = String::new();
        let mut current_seq = String::new();
        for line in text.lines() {
            if line.starts_with('>') {
                if !current_id.is_empty() {
                    let id_line = format!("@{}\n", current_id);
                    let qual_line = qual_char.to_string().repeat(current_seq.len());
                    out.push(id_line.into_bytes());
                    out.push(current_seq.clone().into_bytes());
                    out.push(b"\n+\n".to_vec());
                    out.push(qual_line.into_bytes());
                    out.push(b"\n".to_vec());
                }
                current_id = line[1..].to_string();
                current_seq.clear();
            } else {
                current_seq.push_str(line.trim());
            }
        }
        if !current_id.is_empty() {
            let id_line = format!("@{}\n", current_id);
            let qual_line = qual_char.to_string().repeat(current_seq.len());
            out.push(id_line.into_bytes());
            out.push(current_seq.into_bytes());
            out.push(b"\n+\n".to_vec());
            out.push(qual_line.into_bytes());
            out.push(b"\n".to_vec());
        }
        Ok(Bytes::from(out.concat()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
