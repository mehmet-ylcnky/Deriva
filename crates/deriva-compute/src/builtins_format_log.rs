//! Category K: Log & Observability Formats (13 functions, #383–#395)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
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
fn utf8(b: &[u8]) -> Result<&str, ComputeError> {
    std::str::from_utf8(b).map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))
}

// ---- #383 SyslogParseFn ----
pub struct SyslogParseFn;
impl ComputeFunction for SyslogParseFn {
    fn id(&self) -> FunctionId { fid("syslog_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let fmt = param_str(p, "format").unwrap_or_else(|| "rfc5424".into());
        let mut lines = Vec::new();
        let re5424 = regex::Regex::new(r"^<(\d+)>(\d+) (\S+) (\S+) (\S+) (\S+) (\S+) (\[.*?\]|-) (.*)$").unwrap();
        let re3164 = regex::Regex::new(r"^<(\d+)>(\w{3}\s+\d+\s+[\d:]+) (\S+) (.*)$").unwrap();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let record = if fmt == "rfc5424" {
                if let Some(c) = re5424.captures(line) {
                    let pri: u64 = c[1].parse().unwrap_or(0);
                    serde_json::json!({
                        "priority": pri, "facility": pri / 8, "severity": pri % 8,
                        "version": &c[2], "timestamp": &c[3], "hostname": &c[4],
                        "app_name": &c[5], "procid": &c[6], "msgid": &c[7],
                        "structured_data": if &c[8] == "-" { serde_json::Value::Null } else { serde_json::Value::String(c[8].to_string()) },
                        "message": &c[9]
                    })
                } else {
                    serde_json::json!({"raw": line, "parse_error": true})
                }
            } else {
                if let Some(c) = re3164.captures(line) {
                    let pri: u64 = c[1].parse().unwrap_or(0);
                    serde_json::json!({
                        "priority": pri, "facility": pri / 8, "severity": pri % 8,
                        "timestamp": &c[2], "hostname": &c[3], "message": &c[4]
                    })
                } else {
                    serde_json::json!({"raw": line, "parse_error": true})
                }
            };
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #384 SyslogWriteFn ----
pub struct SyslogWriteFn;
impl ComputeFunction for SyslogWriteFn {
    fn id(&self) -> FunctionId { fid("syslog_write") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let fmt = param_str(p, "format").unwrap_or_else(|| "rfc5424".into());
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let v: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
            let pri = v["priority"].as_u64().unwrap_or(13);
            if fmt == "rfc5424" {
                let ver = v["version"].as_str().unwrap_or("1");
                let ts = v["timestamp"].as_str().unwrap_or("-");
                let host = v["hostname"].as_str().unwrap_or("-");
                let app = v["app_name"].as_str().unwrap_or("-");
                let pid = v["procid"].as_str().unwrap_or("-");
                let mid = v["msgid"].as_str().unwrap_or("-");
                let sd = if v["structured_data"].is_null() { "-".to_string() } else { v["structured_data"].as_str().unwrap_or("-").to_string() };
                let msg = v["message"].as_str().unwrap_or("");
                lines.push(format!("<{pri}>{ver} {ts} {host} {app} {pid} {mid} {sd} {msg}"));
            } else {
                let ts = v["timestamp"].as_str().unwrap_or("");
                let host = v["hostname"].as_str().unwrap_or("");
                let msg = v["message"].as_str().unwrap_or("");
                lines.push(format!("<{pri}>{ts} {host} {msg}"));
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #385 CefParseFn ----
pub struct CefParseFn;
impl ComputeFunction for CefParseFn {
    fn id(&self) -> FunctionId { fid("cef_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            if let Some(rest) = line.strip_prefix("CEF:") {
                let parts: Vec<&str> = rest.splitn(8, '|').collect();
                if parts.len() >= 7 {
                    let mut record = serde_json::json!({
                        "cef_version": parts[0], "vendor": parts[1], "product": parts[2],
                        "device_version": parts[3], "signature_id": parts[4],
                        "name": parts[5], "severity": parts[6]
                    });
                    if parts.len() > 7 {
                        let mut ext = serde_json::Map::new();
                        for kv in parts[7].split_whitespace() {
                            if let Some((k, v)) = kv.split_once('=') {
                                ext.insert(k.to_string(), serde_json::Value::String(v.to_string()));
                            }
                        }
                        record["extensions"] = serde_json::Value::Object(ext);
                    }
                    lines.push(serde_json::to_string(&record).unwrap());
                }
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #386 ElfParseFn (Extended Log Format) ----
pub struct ElfParseFn;
impl ComputeFunction for ElfParseFn {
    fn id(&self) -> FunctionId { fid("elf_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut fields: Vec<String> = Vec::new();
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.starts_with("#Fields:") {
                fields = line.trim_start_matches("#Fields:").trim().split_whitespace().map(|s| s.to_string()).collect();
                continue;
            }
            if line.starts_with('#') || line.is_empty() { continue; }
            let vals: Vec<&str> = line.split_whitespace().collect();
            let mut record = serde_json::Map::new();
            for (i, val) in vals.iter().enumerate() {
                let key = fields.get(i).cloned().unwrap_or_else(|| format!("field_{i}"));
                record.insert(key, serde_json::Value::String(val.to_string()));
            }
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #387 ApacheLogParseFn ----
pub struct ApacheLogParseFn;
impl ComputeFunction for ApacheLogParseFn {
    fn id(&self) -> FunctionId { fid("apache_log_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let fmt = param_str(p, "format").unwrap_or_else(|| "combined".into());
        let re_combined = regex::Regex::new(r#"^(\S+) \S+ (\S+) \[([^\]]+)\] "(\S+) (\S+) (\S+)" (\d+) (\d+|-) "([^"]*)" "([^"]*)""#).unwrap();
        let re_common = regex::Regex::new(r#"^(\S+) \S+ (\S+) \[([^\]]+)\] "(\S+) (\S+) (\S+)" (\d+) (\d+|-)"#).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            if fmt == "combined" {
                if let Some(c) = re_combined.captures(line) {
                    let record = serde_json::json!({
                        "remote_addr": &c[1], "remote_user": &c[2], "timestamp": &c[3],
                        "method": &c[4], "path": &c[5], "protocol": &c[6],
                        "status": &c[7], "body_bytes": &c[8],
                        "referer": &c[9], "user_agent": &c[10]
                    });
                    lines.push(serde_json::to_string(&record).unwrap());
                }
            } else if let Some(c) = re_common.captures(line) {
                let record = serde_json::json!({
                    "remote_addr": &c[1], "remote_user": &c[2], "timestamp": &c[3],
                    "method": &c[4], "path": &c[5], "protocol": &c[6],
                    "status": &c[7], "body_bytes": &c[8]
                });
                lines.push(serde_json::to_string(&record).unwrap());
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #388 NginxLogParseFn ----
pub struct NginxLogParseFn;
impl ComputeFunction for NginxLogParseFn {
    fn id(&self) -> FunctionId { fid("nginx_log_parse") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let fmt = param_str(p, "format").unwrap_or_else(|| "combined".into());
        if fmt == "json" {
            let mut lines = Vec::new();
            for line in text.lines() {
                if line.is_empty() { continue; }
                let _: serde_json::Value = serde_json::from_str(line)
                    .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
                lines.push(line.to_string());
            }
            return Ok(Bytes::from(lines.join("\n")));
        }
        ApacheLogParseFn.execute(inputs, p)
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #389 CloudTrailParseFn ----
pub struct CloudTrailParseFn;
impl ComputeFunction for CloudTrailParseFn {
    fn id(&self) -> FunctionId { fid("cloudtrail_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?;
        let records = v["Records"].as_array()
            .ok_or_else(|| ComputeError::ExecutionFailed("missing Records array".into()))?;
        let lines: Vec<String> = records.iter().map(|r| serde_json::to_string(r).unwrap()).collect();
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #390 VpcFlowLogParseFn ----
pub struct VpcFlowLogParseFn;
impl ComputeFunction for VpcFlowLogParseFn {
    fn id(&self) -> FunctionId { fid("vpc_flow_log_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut iter = text.lines();
        let header = iter.next().ok_or_else(|| ComputeError::ExecutionFailed("empty input".into()))?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        let mut lines = Vec::new();
        for line in iter {
            if line.is_empty() { continue; }
            let vals: Vec<&str> = line.split_whitespace().collect();
            let mut record = serde_json::Map::new();
            for (i, val) in vals.iter().enumerate() {
                let key = fields.get(i).unwrap_or(&"unknown");
                record.insert(key.to_string(), serde_json::Value::String(val.to_string()));
            }
            lines.push(serde_json::to_string(&record).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #391 PrometheusExpositionParseFn ----
pub struct PrometheusExpositionParseFn;
impl ComputeFunction for PrometheusExpositionParseFn {
    fn id(&self) -> FunctionId { fid("prometheus_exposition_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let re = regex::Regex::new(r"^(\w+)(\{([^}]*)\})?\s+([\d.eE+-]+)(?:\s+(\d+))?$").unwrap();
        let mut metrics = Vec::new();
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some(c) = re.captures(line) {
                let name = &c[1];
                let mut labels = serde_json::Map::new();
                if let Some(lbl) = c.get(3) {
                    for pair in lbl.as_str().split(',') {
                        if let Some((k, v)) = pair.split_once('=') {
                            labels.insert(k.trim().to_string(), serde_json::Value::String(v.trim_matches('"').to_string()));
                        }
                    }
                }
                let value: f64 = c[4].parse().unwrap_or(0.0);
                let mut m = serde_json::json!({"name": name, "value": value, "labels": labels});
                if let Some(ts) = c.get(5) { m["timestamp"] = serde_json::Value::String(ts.as_str().to_string()); }
                metrics.push(m);
            }
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&metrics).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #392 OtlpDecodeFn (stub — requires protobuf descriptors) ----
pub struct OtlpDecodeFn;
impl ComputeFunction for OtlpDecodeFn {
    fn id(&self) -> FunctionId { fid("otlp_decode") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let signal = param_str(p, "signal").ok_or_else(|| ComputeError::InvalidParam("missing 'signal' (traces/metrics/logs)".into()))?;
        match signal.as_str() {
            "traces" | "metrics" | "logs" => {}
            _ => return Err(ComputeError::InvalidParam("signal must be traces, metrics, or logs".into())),
        }
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(b) {
            return Ok(Bytes::from(serde_json::to_string_pretty(&v).unwrap()));
        }
        Err(ComputeError::ExecutionFailed("OTLP protobuf decoding requires compiled descriptors; use JSON-encoded OTLP".into()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #393 LogTimestampNormalizeFn ----
pub struct LogTimestampNormalizeFn;
impl ComputeFunction for LogTimestampNormalizeFn {
    fn id(&self) -> FunctionId { fid("log_timestamp_normalize") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let re_iso = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}").unwrap();
        let re_apache = regex::Regex::new(r"\[(\d{2})/(\w{3})/(\d{4}):(\d{2}:\d{2}:\d{2}) ([+-]\d{4})\]").unwrap();
        let re_syslog = regex::Regex::new(r"(\w{3})\s+(\d+)\s+(\d{2}:\d{2}:\d{2})").unwrap();
        let re_epoch = regex::Regex::new(r"(?:^|\s)(\d{10})(?:\s|$)").unwrap();
        let re_slash = regex::Regex::new(r"(\d{4})/(\d{2})/(\d{2}) (\d{2}:\d{2}:\d{2})").unwrap();
        let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let mut v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|_| serde_json::json!({"raw": line}));
            let normalized = if re_iso.is_match(line) {
                re_iso.find(line).map(|m| m.as_str().to_string())
            } else if let Some(c) = re_apache.captures(line) {
                let mon = months.iter().position(|&m| m == &c[2]).map(|i| i + 1).unwrap_or(1);
                Some(format!("{}-{:02}-{}T{}{}", &c[3], mon, &c[1], &c[4], &c[5]))
            } else if let Some(c) = re_slash.captures(line) {
                Some(format!("{}-{}-{}T{}Z", &c[1], &c[2], &c[3], &c[4]))
            } else if let Some(c) = re_epoch.captures(line) {
                let secs: i64 = c[1].parse().unwrap_or(0);
                Some(format!("1970-01-01T00:00:00Z+{secs}s"))
            } else if let Some(c) = re_syslog.captures(line) {
                let mon = months.iter().position(|&m| m == &c[1]).map(|i| i + 1).unwrap_or(1);
                let day: u32 = c[2].parse().unwrap_or(1);
                Some(format!("2024-{:02}-{:02}T{}Z", mon, day, &c[3]))
            } else { None };
            if let Some(ts) = normalized {
                v["normalized_timestamp"] = serde_json::Value::String(ts);
                v["normalized"] = serde_json::Value::Bool(true);
            } else {
                v["normalized"] = serde_json::Value::Bool(false);
            }
            lines.push(serde_json::to_string(&v).unwrap());
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #394 LogLevelFilterFn ----
pub struct LogLevelFilterFn;
fn level_rank(l: &str) -> u8 {
    match l.to_lowercase().as_str() {
        "trace" | "debug" => 0,
        "info" => 1,
        "warn" | "warning" => 2,
        "error" | "err" => 3,
        "fatal" | "critical" | "crit" => 4,
        _ => 0,
    }
}
impl ComputeFunction for LogLevelFilterFn {
    fn id(&self) -> FunctionId { fid("log_level_filter") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let min = param_str(p, "min_level").unwrap_or_else(|| "debug".into());
        let min_rank = level_rank(&min);
        let re = regex::Regex::new(r"(?i)\b(trace|debug|info|warn(?:ing)?|error|err|fatal|critical|crit)\b").unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(lvl) = v.get("level").and_then(|l| l.as_str()) {
                    if level_rank(lvl) >= min_rank { lines.push(line.to_string()); }
                    continue;
                }
            }
            if let Some(m) = re.find(line) {
                if level_rank(m.as_str()) >= min_rank { lines.push(line.to_string()); }
            } else {
                lines.push(line.to_string());
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #395 LogAnonymizeFn ----
pub struct LogAnonymizeFn;
fn hash8(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", h.finish() & 0xFFFFFFFF)
}
impl ComputeFunction for LogAnonymizeFn {
    fn id(&self) -> FunctionId { fid("log_anonymize") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let fields_str = param_str(p, "fields").unwrap_or_else(|| "ip,email".into());
        let fields: Vec<&str> = fields_str.split(',').map(|s| s.trim()).collect();
        let re_ip = regex::Regex::new(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b").unwrap();
        let re_email = regex::Regex::new(r"\b([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})\b").unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() { continue; }
            let mut out = line.to_string();
            if fields.contains(&"ip") {
                out = re_ip.replace_all(&out, |caps: &regex::Captures| format!("REDACTED_IP_{}", hash8(&caps[1]))).to_string();
            }
            if fields.contains(&"email") {
                out = re_email.replace_all(&out, |caps: &regex::Captures| format!("REDACTED_EMAIL_{}", hash8(&caps[1]))).to_string();
            }
            if fields.contains(&"user") {
                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&out) {
                    for key in ["user", "remote_user", "username"] {
                        if let Some(u) = v.get(key).and_then(|u| u.as_str()).map(|s| s.to_string()) {
                            if u != "-" { v[key] = serde_json::Value::String(format!("REDACTED_USER_{}", hash8(&u))); }
                        }
                    }
                    out = serde_json::to_string(&v).unwrap();
                }
            }
            lines.push(out);
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
