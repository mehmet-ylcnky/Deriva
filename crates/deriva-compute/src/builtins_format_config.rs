//! Category H: Configuration & Schema Formats — YAML, TOML, INI, HCL (16 functions, #340–#355)

use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;

fn fid(name: &str) -> FunctionId {
    FunctionId { name: name.to_string(), version: "1.0.0".to_string() }
}
fn utf8(b: &[u8]) -> Result<&str, ComputeError> {
    std::str::from_utf8(b).map_err(|_| ComputeError::ExecutionFailed("requires UTF-8".into()))
}
fn param(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    if inputs.len() != 1 { return Err(ComputeError::InputCount { expected: 1, got: inputs.len() }); }
    Ok(&inputs[0])
}
fn cost(sizes: &[u64]) -> ComputeCost {
    ComputeCost { cpu_ms: sizes.iter().sum::<u64>() / 100 + 50, memory_bytes: sizes.iter().sum::<u64>() * 2 + 1024 }
}

// ---- #340 YamlParseFn ----
pub struct YamlParseFn;
impl ComputeFunction for YamlParseFn {
    fn id(&self) -> FunctionId { fid("yaml_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_yaml::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("yaml parse: {e}")))?;
        Ok(Bytes::from(serde_json::to_string_pretty(&v).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #341 YamlWriteFn ----
pub struct YamlWriteFn;
impl ComputeFunction for YamlWriteFn {
    fn id(&self) -> FunctionId { fid("yaml_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let out = serde_yaml::to_string(&v)
            .map_err(|e| ComputeError::ExecutionFailed(format!("yaml write: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #342 YamlValidateFn ----
pub struct YamlValidateFn;
impl ComputeFunction for YamlValidateFn {
    fn id(&self) -> FunctionId { fid("yaml_validate") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let _: serde_yaml::Value = serde_yaml::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("yaml invalid: {e}")))?;
        Ok(b.clone())
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #343 YamlMergeFn ----
pub struct YamlMergeFn;
fn deep_merge_yaml(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o { deep_merge_yaml(b.entry(k).or_insert(serde_json::Value::Null), v); }
        }
        (serde_json::Value::Array(b), serde_json::Value::Array(o)) => b.extend(o.iter().cloned()),
        (b, o) => *b = o.clone(),
    }
}
impl ComputeFunction for YamlMergeFn {
    fn id(&self) -> FunctionId { fid("yaml_merge") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 2, got: 0 }); }
        let deep = param(p, "strategy").as_deref() != Some("shallow");
        let mut merged: serde_json::Value = serde_yaml::from_slice(&inputs[0])
            .map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {e}")))?;
        for inp in &inputs[1..] {
            let v: serde_json::Value = serde_yaml::from_slice(inp)
                .map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {e}")))?;
            if deep {
                deep_merge_yaml(&mut merged, &v);
            } else if let (serde_json::Value::Object(b), serde_json::Value::Object(o)) = (&mut merged, &v) {
                for (k, val) in o { b.insert(k.clone(), val.clone()); }
            }
        }
        Ok(Bytes::from(serde_yaml::to_string(&merged).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #344 TomlParseFn ----
pub struct TomlParseFn;
impl ComputeFunction for TomlParseFn {
    fn id(&self) -> FunctionId { fid("toml_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = toml::from_str(utf8(b)?)
            .map_err(|e| ComputeError::ExecutionFailed(format!("toml parse: {e}")))?;
        Ok(Bytes::from(serde_json::to_string_pretty(&v).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #345 TomlWriteFn ----
pub struct TomlWriteFn;
impl ComputeFunction for TomlWriteFn {
    fn id(&self) -> FunctionId { fid("toml_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let out = toml::to_string_pretty(&v)
            .map_err(|e| ComputeError::ExecutionFailed(format!("toml write: {e}")))?;
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #346 IniParseFn ----
pub struct IniParseFn;
impl ComputeFunction for IniParseFn {
    fn id(&self) -> FunctionId { fid("ini_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut config = configparser::ini::Ini::new();
        config.read(text.to_string())
            .map_err(|e| ComputeError::ExecutionFailed(format!("ini parse: {e}")))?;
        let mut root = serde_json::Map::new();
        for section in config.sections() {
            let mut obj = serde_json::Map::new();
            if let Some(map) = config.get_map_ref().get(&section) {
                for (key, val) in map {
                    obj.insert(key.clone(), match val {
                        Some(v) => serde_json::Value::String(v.clone()),
                        None => serde_json::Value::Null,
                    });
                }
            }
            root.insert(section, serde_json::Value::Object(obj));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&root).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #347 IniWriteFn ----
pub struct IniWriteFn;
impl ComputeFunction for IniWriteFn {
    fn id(&self) -> FunctionId { fid("ini_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let obj = v.as_object().ok_or_else(|| ComputeError::ExecutionFailed("expected JSON object".into()))?;
        let mut out = String::new();
        for (section, vals) in obj {
            out.push_str(&format!("[{section}]\n"));
            if let Some(map) = vals.as_object() {
                for (k, val) in map {
                    let s = match val { serde_json::Value::String(s) => s.clone(), other => other.to_string() };
                    out.push_str(&format!("{k} = {s}\n"));
                }
            }
            out.push('\n');
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #348 EnvParseFn ----
pub struct EnvParseFn;
impl ComputeFunction for EnvParseFn {
    fn id(&self) -> FunctionId { fid("env_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut map = serde_json::Map::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
            let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed).trim();
            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                let val = if (val.starts_with('"') && val.ends_with('"')) || (val.starts_with('\'') && val.ends_with('\'')) {
                    &val[1..val.len()-1]
                } else { val };
                map.insert(key.to_string(), serde_json::Value::String(val.to_string()));
            }
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&map).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #349 EnvWriteFn ----
pub struct EnvWriteFn;
impl ComputeFunction for EnvWriteFn {
    fn id(&self) -> FunctionId { fid("env_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let obj = v.as_object().ok_or_else(|| ComputeError::ExecutionFailed("expected JSON object".into()))?;
        let mut out = String::new();
        for (k, val) in obj {
            let s = match val { serde_json::Value::String(s) => s.clone(), other => other.to_string() };
            if s.contains(' ') || s.contains('"') || s.contains('\'') {
                out.push_str(&format!("{k}=\"{s}\"\n"));
            } else {
                out.push_str(&format!("{k}={s}\n"));
            }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #350 HclParseFn ----
pub struct HclParseFn;
impl ComputeFunction for HclParseFn {
    fn id(&self) -> FunctionId { fid("hcl_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let body: hcl::Body = hcl::from_str(text)
            .map_err(|e| ComputeError::ExecutionFailed(format!("hcl parse: {e}")))?;
        let json = hcl_body_to_json(&body);
        Ok(Bytes::from(serde_json::to_string_pretty(&json).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

fn hcl_body_to_json(body: &hcl::Body) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for attr in body.attributes() {
        map.insert(attr.key.to_string(), hcl_expr_to_json(attr.expr()));
    }
    for block in body.blocks() {
        let label = if block.labels.is_empty() {
            block.identifier.to_string()
        } else {
            format!("{}.{}", block.identifier, block.labels.iter().map(|l| l.as_str()).collect::<Vec<_>>().join("."))
        };
        map.insert(label, hcl_body_to_json(&block.body));
    }
    serde_json::Value::Object(map)
}

fn hcl_expr_to_json(expr: &hcl::Expression) -> serde_json::Value {
    match expr {
        hcl::Expression::String(s) => serde_json::Value::String(s.clone()),
        hcl::Expression::Number(n) => serde_json::json!(n.as_f64().unwrap_or(0.0)),
        hcl::Expression::Bool(b) => serde_json::Value::Bool(*b),
        hcl::Expression::Null => serde_json::Value::Null,
        hcl::Expression::Array(arr) => serde_json::Value::Array(arr.iter().map(hcl_expr_to_json).collect()),
        hcl::Expression::Object(obj) => {
            let mut m = serde_json::Map::new();
            for (k, v) in obj {
                m.insert(k.to_string(), hcl_expr_to_json(v));
            }
            serde_json::Value::Object(m)
        }
        other => serde_json::Value::String(other.to_string()),
    }
}

// ---- #351 PropertiesParseFn ----
pub struct PropertiesParseFn;
impl ComputeFunction for PropertiesParseFn {
    fn id(&self) -> FunctionId { fid("properties_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        let mut map = serde_json::Map::new();
        let mut continuation = String::new();
        let mut cont_key = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') { continue; }
            if !continuation.is_empty() {
                if trimmed.ends_with('\\') {
                    continuation.push_str(trimmed.strip_suffix('\\').unwrap_or(trimmed).trim());
                } else {
                    continuation.push_str(trimmed);
                    map.insert(cont_key.clone(), serde_json::Value::String(continuation.clone()));
                    continuation.clear();
                }
                continue;
            }
            let sep = trimmed.find(['=', ':']).unwrap_or(trimmed.len());
            let key = trimmed[..sep].trim().to_string();
            let val = if sep < trimmed.len() { trimmed[sep+1..].trim() } else { "" };
            if val.ends_with('\\') {
                cont_key = key;
                continuation = val.strip_suffix('\\').unwrap_or(val).trim().to_string();
            } else {
                map.insert(key, serde_json::Value::String(val.to_string()));
            }
        }
        if !continuation.is_empty() {
            map.insert(cont_key, serde_json::Value::String(continuation));
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&map).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #352 PropertiesWriteFn ----
pub struct PropertiesWriteFn;
impl ComputeFunction for PropertiesWriteFn {
    fn id(&self) -> FunctionId { fid("properties_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let obj = v.as_object().ok_or_else(|| ComputeError::ExecutionFailed("expected JSON object".into()))?;
        let mut out = String::new();
        for (k, val) in obj {
            let s = match val { serde_json::Value::String(s) => s.clone(), other => other.to_string() };
            out.push_str(&format!("{k}={s}\n"));
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #353 PlistParseFn ----
pub struct PlistParseFn;
impl ComputeFunction for PlistParseFn {
    fn id(&self) -> FunctionId { fid("plist_parse") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let text = utf8(b)?;
        // Simple XML plist parser — extracts <key>/<string>/<integer>/<true/>/<false/> pairs
        if !text.contains("<plist") {
            return Err(ComputeError::ExecutionFailed("only XML plist supported".into()));
        }
        let mut map = serde_json::Map::new();
        let mut key: Option<String> = None;
        for line in text.lines() {
            let t = line.trim();
            if let Some(k) = t.strip_prefix("<key>").and_then(|s| s.strip_suffix("</key>")) {
                key = Some(k.to_string());
            } else if let Some(ref k) = key {
                let val = if let Some(s) = t.strip_prefix("<string>").and_then(|s| s.strip_suffix("</string>")) {
                    serde_json::Value::String(s.to_string())
                } else if let Some(s) = t.strip_prefix("<integer>").and_then(|s| s.strip_suffix("</integer>")) {
                    serde_json::json!(s.parse::<i64>().unwrap_or(0))
                } else if let Some(s) = t.strip_prefix("<real>").and_then(|s| s.strip_suffix("</real>")) {
                    serde_json::json!(s.parse::<f64>().unwrap_or(0.0))
                } else if t == "<true/>" {
                    serde_json::Value::Bool(true)
                } else if t == "<false/>" {
                    serde_json::Value::Bool(false)
                } else { continue };
                map.insert(k.clone(), val);
                key = None;
            }
        }
        Ok(Bytes::from(serde_json::to_string_pretty(&map).unwrap()))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #354 PlistWriteFn ----
pub struct PlistWriteFn;
impl ComputeFunction for PlistWriteFn {
    fn id(&self) -> FunctionId { fid("plist_write") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let v: serde_json::Value = serde_json::from_slice(b)
            .map_err(|e| ComputeError::ExecutionFailed(format!("json parse: {e}")))?;
        let obj = v.as_object().ok_or_else(|| ComputeError::ExecutionFailed("expected JSON object".into()))?;
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n");
        for (k, val) in obj {
            out.push_str(&format!("  <key>{k}</key>\n"));
            match val {
                serde_json::Value::String(s) => out.push_str(&format!("  <string>{s}</string>\n")),
                serde_json::Value::Number(n) => {
                    if n.is_f64() { out.push_str(&format!("  <real>{}</real>\n", n)); }
                    else { out.push_str(&format!("  <integer>{}</integer>\n", n)); }
                }
                serde_json::Value::Bool(true) => out.push_str("  <true/>\n"),
                serde_json::Value::Bool(false) => out.push_str("  <false/>\n"),
                _ => out.push_str(&format!("  <string>{}</string>\n", val)),
            }
        }
        out.push_str("</dict>\n</plist>\n");
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}

// ---- #355 ConfigFormatConvertFn ----
pub struct ConfigFormatConvertFn;
impl ComputeFunction for ConfigFormatConvertFn {
    fn id(&self) -> FunctionId { fid("config_format_convert") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let from = param(p, "from").ok_or_else(|| ComputeError::InvalidParam("missing 'from'".into()))?;
        let to = param(p, "to").ok_or_else(|| ComputeError::InvalidParam("missing 'to'".into()))?;
        // Step 1: parse → JSON
        let v: serde_json::Value = match from.as_str() {
            "yaml" => serde_yaml::from_slice(b).map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {e}")))?,
            "toml" => toml::from_str(utf8(b)?).map_err(|e| ComputeError::ExecutionFailed(format!("toml: {e}")))?,
            "json" => serde_json::from_slice(b).map_err(|e| ComputeError::ExecutionFailed(format!("json: {e}")))?,
            "ini" => { let r = IniParseFn.execute(inputs.clone(), &BTreeMap::new())?; serde_json::from_slice(&r).unwrap() }
            "env" => { let r = EnvParseFn.execute(inputs.clone(), &BTreeMap::new())?; serde_json::from_slice(&r).unwrap() }
            "properties" => { let r = PropertiesParseFn.execute(inputs.clone(), &BTreeMap::new())?; serde_json::from_slice(&r).unwrap() }
            other => return Err(ComputeError::InvalidParam(format!("unsupported source: {other}"))),
        };
        // Step 2: JSON → target
        match to.as_str() {
            "yaml" => Ok(Bytes::from(serde_yaml::to_string(&v).map_err(|e| ComputeError::ExecutionFailed(format!("yaml: {e}")))?)),
            "toml" => Ok(Bytes::from(toml::to_string_pretty(&v).map_err(|e| ComputeError::ExecutionFailed(format!("toml: {e}")))?)),
            "json" => Ok(Bytes::from(serde_json::to_string_pretty(&v).unwrap())),
            "env" => EnvWriteFn.execute(vec![Bytes::from(serde_json::to_vec(&v).unwrap())], &BTreeMap::new()),
            "ini" => IniWriteFn.execute(vec![Bytes::from(serde_json::to_vec(&v).unwrap())], &BTreeMap::new()),
            "properties" => PropertiesWriteFn.execute(vec![Bytes::from(serde_json::to_vec(&v).unwrap())], &BTreeMap::new()),
            other => Err(ComputeError::InvalidParam(format!("unsupported target: {other}"))),
        }
    }
    fn estimated_cost(&self, s: &[u64]) -> ComputeCost { cost(s) }
}
