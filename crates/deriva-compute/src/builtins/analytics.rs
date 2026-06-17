use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::{get_string_param, parse_usize_param, spec_cost};

// ── Helper: parse newline-separated numeric lines ──

fn parse_numeric_lines(input: &[u8]) -> Result<Vec<f64>, ComputeError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| ComputeError::ExecutionFailed("input must be UTF-8".into()))?;
    let mut values = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let n: f64 = trimmed.parse().map_err(|_| {
            ComputeError::ExecutionFailed(format!("not a number: '{}'", trimmed))
        })?;
        values.push(n);
    }
    Ok(values)
}

// ── ReservoirSampleFn ──

pub struct ReservoirSampleFn;

impl ComputeFunction for ReservoirSampleFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("reservoir_sample", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        use rand::Rng;
        use rand::SeedableRng;

        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let k: usize = parse_usize_param(params, "k")?;
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("reservoir_sample requires UTF-8 input".into()))?;
        let lines: Vec<&str> = text.lines().collect();

        if k >= lines.len() {
            return Ok(Bytes::from(lines.join("\n")));
        }

        // Determine seed: use provided seed or default to 0 for determinism
        let seed: u64 = match params.get("seed") {
            Some(Value::String(s)) => s.parse::<u64>().map_err(|_| {
                ComputeError::InvalidParam("seed must be a non-negative integer".into())
            })?,
            Some(Value::Int(n)) => u64::try_from(*n).map_err(|_| {
                ComputeError::InvalidParam("seed must be non-negative".into())
            })?,
            None => 0,
            _ => return Err(ComputeError::InvalidParam("seed must be a string or integer".into())),
        };

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut reservoir: Vec<usize> = (0..k).collect();
        for i in k..lines.len() {
            let j = rng.gen_range(0..=i);
            if j < k {
                reservoir[j] = i;
            }
        }
        reservoir.sort();
        let sampled: Vec<&str> = reservoir.iter().map(|&i| lines[i]).collect();
        Ok(Bytes::from(sampled.join("\n")))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── PercentileFn ──

pub struct PercentileFn;

impl ComputeFunction for PercentileFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("percentile", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let p_str = get_string_param(params, "p")?;
        let p: f64 = p_str.parse().map_err(|_| {
            ComputeError::InvalidParam("p must be a number between 0 and 100".into())
        })?;
        if !(0.0..=100.0).contains(&p) {
            return Err(ComputeError::InvalidParam(
                "p must be between 0 and 100".into(),
            ));
        }

        let mut values = parse_numeric_lines(&inputs[0])?;
        if values.is_empty() {
            return Err(ComputeError::ExecutionFailed(
                "percentile requires at least one numeric value".into(),
            ));
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Linear interpolation method
        let n = values.len();
        if n == 1 {
            return Ok(Bytes::from(values[0].to_string()));
        }
        let rank = (p / 100.0) * (n as f64 - 1.0);
        let lower = rank.floor() as usize;
        let upper = rank.ceil() as usize;
        let frac = rank - rank.floor();
        let result = values[lower] * (1.0 - frac) + values[upper] * frac;
        Ok(Bytes::from(result.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(100, input_sizes)
    }
}

// ── MeanFn ──

pub struct MeanFn;

impl ComputeFunction for MeanFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("mean", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let values = parse_numeric_lines(&inputs[0])?;
        if values.is_empty() {
            return Err(ComputeError::ExecutionFailed(
                "mean requires at least one numeric value".into(),
            ));
        }
        let sum: f64 = values.iter().sum();
        let mean = sum / values.len() as f64;
        Ok(Bytes::from(mean.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── MedianFn ──

pub struct MedianFn;

impl ComputeFunction for MedianFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("median", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut values = parse_numeric_lines(&inputs[0])?;
        if values.is_empty() {
            return Err(ComputeError::ExecutionFailed(
                "median requires at least one numeric value".into(),
            ));
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        let median = if n % 2 == 0 {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        } else {
            values[n / 2]
        };
        Ok(Bytes::from(median.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(100, input_sizes)
    }
}

// ── MinFn (numeric line-based) ──

pub struct NumericMinFn;

impl ComputeFunction for NumericMinFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("min", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let values = parse_numeric_lines(&inputs[0])?;
        if values.is_empty() {
            return Err(ComputeError::ExecutionFailed(
                "min requires at least one numeric value".into(),
            ));
        }
        let min = values
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        Ok(Bytes::from(min.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── MaxFn (numeric line-based) ──

pub struct NumericMaxFn;

impl ComputeFunction for NumericMaxFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("max", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let values = parse_numeric_lines(&inputs[0])?;
        if values.is_empty() {
            return Err(ComputeError::ExecutionFailed(
                "max requires at least one numeric value".into(),
            ));
        }
        let max = values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(Bytes::from(max.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── FrequencyFn ──

pub struct FrequencyFn;

impl ComputeFunction for FrequencyFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("frequency", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("frequency requires UTF-8 input".into()))?;

        let mut counts: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            *counts.entry(line).or_insert(0) += 1;
        }

        // Sort by count descending, then by value ascending for stability
        let mut entries: Vec<(&str, u64)> = counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

        // Build JSON object preserving the sorted order
        let mut json = String::from("{");
        for (i, (key, count)) in entries.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            // Escape JSON special characters in key
            for c in key.chars() {
                match c {
                    '"' => json.push_str("\\\""),
                    '\\' => json.push_str("\\\\"),
                    '\n' => json.push_str("\\n"),
                    '\r' => json.push_str("\\r"),
                    '\t' => json.push_str("\\t"),
                    _ => json.push(c),
                }
            }
            json.push_str("\":");
            json.push_str(&count.to_string());
        }
        json.push('}');

        Ok(Bytes::from(json))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(100, input_sizes)
    }
}

// ── DedupCountFn ──

pub struct DedupCountFn;

impl ComputeFunction for DedupCountFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("dedup_count", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let text = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("dedup_count requires UTF-8 input".into()))?;

        let mut unique: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            unique.insert(line);
        }

        Ok(Bytes::from(unique.len().to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── CardinalityFn ──

pub struct CardinalityFn;

impl ComputeFunction for CardinalityFn {
    fn id(&self) -> FunctionId {
        FunctionId::new("cardinality", "1.0.0")
    }

    fn execute(
        &self,
        inputs: Vec<Bytes>,
        _params: &BTreeMap<String, Value>,
    ) -> Result<Bytes, ComputeError> {
        if inputs.len() != 1 {
            return Err(ComputeError::InputCount { expected: 1, got: inputs.len() });
        }
        let mut seen = [false; 256];
        for &b in inputs[0].iter() {
            seen[b as usize] = true;
        }
        let count = seen.iter().filter(|&&s| s).count();
        Ok(Bytes::from(count.to_string()))
    }

    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        spec_cost(50, input_sizes)
    }
}

// ── Unit tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::ComputeFunction;

    fn make_params(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect()
    }

    fn exec_one(f: &dyn ComputeFunction, input: &[u8], params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        f.execute(vec![Bytes::from(input.to_vec())], params)
    }

    // ── ReservoirSampleFn tests ──

    #[test]
    fn test_reservoir_sample_basic() {
        let f = ReservoirSampleFn;
        let input = b"a\nb\nc\nd\ne";
        let params = make_params(&[("k", "3"), ("seed", "42")]);
        let result = exec_one(&f, input, &params).unwrap();
        let text = std::str::from_utf8(&result).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_reservoir_sample_deterministic() {
        let f = ReservoirSampleFn;
        let input = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let params = make_params(&[("k", "3"), ("seed", "123")]);
        let r1 = exec_one(&f, input, &params).unwrap();
        let r2 = exec_one(&f, input, &params).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_reservoir_sample_k_larger_than_input() {
        let f = ReservoirSampleFn;
        let input = b"a\nb";
        let params = make_params(&[("k", "5"), ("seed", "1")]);
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"a\nb");
    }

    #[test]
    fn test_reservoir_sample_input_count_error() {
        let f = ReservoirSampleFn;
        let params = make_params(&[("k", "3")]);
        let err = f.execute(vec![], &params).unwrap_err();
        assert!(matches!(err, ComputeError::InputCount { expected: 1, got: 0 }));
    }

    // ── PercentileFn tests ──

    #[test]
    fn test_percentile_median() {
        let f = PercentileFn;
        let input = b"1\n2\n3\n4\n5";
        let params = make_params(&[("p", "50")]);
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_percentile_0() {
        let f = PercentileFn;
        let input = b"10\n20\n30";
        let params = make_params(&[("p", "0")]);
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_percentile_100() {
        let f = PercentileFn;
        let input = b"10\n20\n30";
        let params = make_params(&[("p", "100")]);
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 30.0).abs() < 1e-10);
    }

    #[test]
    fn test_percentile_invalid_p() {
        let f = PercentileFn;
        let input = b"1\n2\n3";
        let params = make_params(&[("p", "101")]);
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::InvalidParam(_)));
    }

    #[test]
    fn test_percentile_unparseable_input() {
        let f = PercentileFn;
        let input = b"1\nabc\n3";
        let params = make_params(&[("p", "50")]);
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::ExecutionFailed(_)));
    }

    // ── MeanFn tests ──

    #[test]
    fn test_mean_basic() {
        let f = MeanFn;
        let input = b"1\n2\n3\n4\n5";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_mean_empty() {
        let f = MeanFn;
        let input = b"";
        let params = BTreeMap::new();
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::ExecutionFailed(_)));
    }

    #[test]
    fn test_mean_unparseable() {
        let f = MeanFn;
        let input = b"1\nfoo\n3";
        let params = BTreeMap::new();
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::ExecutionFailed(_)));
    }

    // ── MedianFn tests ──

    #[test]
    fn test_median_odd() {
        let f = MedianFn;
        let input = b"3\n1\n2";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_median_even() {
        let f = MedianFn;
        let input = b"1\n2\n3\n4";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 2.5).abs() < 1e-10);
    }

    // ── NumericMinFn tests ──

    #[test]
    fn test_min_basic() {
        let f = NumericMinFn;
        let input = b"5\n3\n8\n1\n4";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_min_negative() {
        let f = NumericMinFn;
        let input = b"-5\n3\n-8\n1";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - (-8.0)).abs() < 1e-10);
    }

    #[test]
    fn test_min_empty() {
        let f = NumericMinFn;
        let input = b"";
        let params = BTreeMap::new();
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::ExecutionFailed(_)));
    }

    // ── NumericMaxFn tests ──

    #[test]
    fn test_max_basic() {
        let f = NumericMaxFn;
        let input = b"5\n3\n8\n1\n4";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let val: f64 = std::str::from_utf8(&result).unwrap().parse().unwrap();
        assert!((val - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_max_empty() {
        let f = NumericMaxFn;
        let input = b"";
        let params = BTreeMap::new();
        let err = exec_one(&f, input, &params).unwrap_err();
        assert!(matches!(err, ComputeError::ExecutionFailed(_)));
    }

    // ── FrequencyFn tests ──

    #[test]
    fn test_frequency_basic() {
        let f = FrequencyFn;
        let input = b"apple\nbanana\napple\napple\nbanana";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        let text = std::str::from_utf8(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(json["apple"], 3);
        assert_eq!(json["banana"], 2);
    }

    #[test]
    fn test_frequency_empty() {
        let f = FrequencyFn;
        let input = b"";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"{}");
    }

    // ── DedupCountFn tests ──

    #[test]
    fn test_dedup_count_basic() {
        let f = DedupCountFn;
        let input = b"apple\nbanana\napple\ncherry\nbanana";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"3");
    }

    #[test]
    fn test_dedup_count_all_unique() {
        let f = DedupCountFn;
        let input = b"a\nb\nc";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"3");
    }

    #[test]
    fn test_dedup_count_empty() {
        let f = DedupCountFn;
        let input = b"";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"0");
    }

    // ── CardinalityFn tests ──

    #[test]
    fn test_cardinality_basic() {
        let f = CardinalityFn;
        let input = b"hello";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        // 'h', 'e', 'l', 'o' = 4 distinct bytes
        assert_eq!(&result[..], b"4");
    }

    #[test]
    fn test_cardinality_all_same() {
        let f = CardinalityFn;
        let input = b"aaaa";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"1");
    }

    #[test]
    fn test_cardinality_empty() {
        let f = CardinalityFn;
        let input = b"";
        let params = BTreeMap::new();
        let result = exec_one(&f, input, &params).unwrap();
        assert_eq!(&result[..], b"0");
    }

    #[test]
    fn test_cardinality_all_256() {
        let f = CardinalityFn;
        let input: Vec<u8> = (0..=255u8).collect();
        let params = BTreeMap::new();
        let result = f.execute(vec![Bytes::from(input)], &params).unwrap();
        assert_eq!(&result[..], b"256");
    }
}
