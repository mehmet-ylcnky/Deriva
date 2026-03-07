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

// ---- #433 PcapReadFn (requires pcap-parser) ----
pub struct PcapReadFn;
impl ComputeFunction for PcapReadFn {
    fn id(&self) -> FunctionId { fid("pcap_read") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("pcap_read requires format-network-pcap feature (pcap-parser)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #434 PcapFilterFn (requires pcap-parser) ----
pub struct PcapFilterFn;
impl ComputeFunction for PcapFilterFn {
    fn id(&self) -> FunctionId { fid("pcap_filter") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("pcap_filter requires format-network-pcap feature (pcap-parser)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #435 PcapStatisticsFn (requires pcap-parser) ----
pub struct PcapStatisticsFn;
impl ComputeFunction for PcapStatisticsFn {
    fn id(&self) -> FunctionId { fid("pcap_statistics") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("pcap_statistics requires format-network-pcap feature (pcap-parser)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #436 DnsRecordParseFn ----
pub struct DnsRecordParseFn;
impl ComputeFunction for DnsRecordParseFn {
    fn id(&self) -> FunctionId { fid("dns_record_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') { continue; }
            if trimmed.starts_with('$') { continue; } // Skip directives
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() < 4 { continue; }
            let record = serde_json::json!({
                "name": parts[0],
                "ttl": parts[1].parse::<u32>().ok(),
                "class": parts[2],
                "type": parts[3],
                "rdata": parts[4..].join(" "),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #437 HarParseFn ----
pub struct HarParseFn;
impl ComputeFunction for HarParseFn {
    fn id(&self) -> FunctionId { fid("har_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let summary = param_str(p, "summary").unwrap_or_else(|| "false".into()) == "true";
        let har: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| fail(format!("har json: {e}")))?;
        let entries = har["log"]["entries"].as_array()
            .ok_or_else(|| fail("missing log.entries".into()))?;
        if summary {
            let total_requests = entries.len();
            let total_bytes: u64 = entries.iter()
                .filter_map(|e| e["response"]["bodySize"].as_u64())
                .sum();
            let result = serde_json::json!({
                "total_requests": total_requests,
                "total_bytes": total_bytes,
            });
            Ok(Bytes::from(serde_json::to_string_pretty(&result).unwrap()))
        } else {
            let mut lines = Vec::new();
            for entry in entries {
                let record = serde_json::json!({
                    "url": entry["request"]["url"],
                    "method": entry["request"]["method"],
                    "status": entry["response"]["status"],
                    "time": entry["time"],
                });
                lines.push(serde_json::to_string(&record).unwrap());
            }
            Ok(Bytes::from(lines.join("\n")))
        }
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #438 EmailParseFn (requires mailparse) ----
pub struct EmailParseFn;
impl ComputeFunction for EmailParseFn {
    fn id(&self) -> FunctionId { fid("email_parse") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("email_parse requires format-network-email feature (mailparse)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #439 EmailExtractAttachmentsFn (requires mailparse) ----
pub struct EmailExtractAttachmentsFn;
impl ComputeFunction for EmailExtractAttachmentsFn {
    fn id(&self) -> FunctionId { fid("email_extract_attachments") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("email_extract_attachments requires format-network-email feature (mailparse)".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #440 MboxSplitFn ----
pub struct MboxSplitFn;
impl ComputeFunction for MboxSplitFn {
    fn id(&self) -> FunctionId { fid("mbox_split") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = std::str::from_utf8(b).map_err(|_| fail("requires UTF-8".into()))?;
        let mut lines = Vec::new();
        let mut current_from = String::new();
        let mut message_size = 0;
        for line in text.lines() {
            if line.starts_with("From ") {
                if !current_from.is_empty() {
                    let record = serde_json::json!({
                        "from": current_from,
                        "size": message_size,
                    });
                    lines.push(serde_json::to_string(&record).unwrap());
                }
                current_from = line[5..].to_string();
                message_size = 0;
            } else {
                message_size += line.len() + 1;
            }
        }
        if !current_from.is_empty() {
            let record = serde_json::json!({
                "from": current_from,
                "size": message_size,
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
