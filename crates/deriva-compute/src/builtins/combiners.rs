use crate::function::{ComputeCost, ComputeError, ComputeFunction};
use bytes::Bytes;
use deriva_core::address::{FunctionId, Value};
use std::collections::BTreeMap;
use super::spec_cost;
use super::parse_usize_param;
use super::get_string_param;

pub struct InterleaveFn;

impl ComputeFunction for InterleaveFn {
    fn id(&self) -> FunctionId { FunctionId::new("interleave", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let bs: usize = match params.get("block_size") {
            Some(Value::String(s)) => s.parse().map_err(|_| ComputeError::InvalidParam("block_size must be positive integer".into()))?,
            None => 1,
            _ => return Err(ComputeError::InvalidParam("block_size must be a string".into())),
        };
        if bs == 0 { return Err(ComputeError::InvalidParam("block_size must be > 0".into())); }
        let mut offsets = vec![0usize; inputs.len()];
        let total: usize = inputs.iter().map(|i| i.len()).sum();
        let mut out = Vec::with_capacity(total);
        loop {
            let mut progress = false;
            for (i, input) in inputs.iter().enumerate() {
                let start = offsets[i];
                if start < input.len() {
                    let end = (start + bs).min(input.len());
                    out.extend_from_slice(&input[start..end]);
                    offsets[i] = end;
                    progress = true;
                }
            }
            if !progress { break; }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #51 ZipConcatFn ──

pub struct ZipConcatFn;

impl ComputeFunction for ZipConcatFn {
    fn id(&self) -> FunctionId { FunctionId::new("zip_concat", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        // Spec 9.3: length-prefix header (4 bytes BE for each input length, then data in order)
        let len_a = inputs[0].len() as u32;
        let len_b = inputs[1].len() as u32;
        let total = 8 + inputs[0].len() + inputs[1].len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&len_a.to_be_bytes());
        out.extend_from_slice(&len_b.to_be_bytes());
        out.extend_from_slice(&inputs[0]);
        out.extend_from_slice(&inputs[1]);
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #52 DiffFn ──

pub struct DiffFn;

impl ComputeFunction for DiffFn {
    fn id(&self) -> FunctionId { FunctionId::new("diff", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        // Spec 9.4: unified diff for line-based text
        let old_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("diff requires UTF-8 input".into()))?;
        let new_str = std::str::from_utf8(&inputs[1])
            .map_err(|_| ComputeError::ExecutionFailed("diff requires UTF-8 input".into()))?;

        let old_lines: Vec<&str> = old_str.lines().collect();
        let new_lines: Vec<&str> = new_str.lines().collect();

        // Compute LCS-based unified diff
        let diff = unified_diff(&old_lines, &new_lines);
        Ok(Bytes::from(diff))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

/// Compute unified diff output from two line slices using a simple LCS algorithm.
fn unified_diff(old: &[&str], new: &[&str]) -> String {
    // Compute the edit script using Myers-like algorithm (simplified LCS approach)
    let lcs = compute_lcs(old, new);

    // Build unified diff hunks from the LCS
    let mut output = String::new();
    output.push_str("--- a\n");
    output.push_str("+++ b\n");

    let hunks = build_hunks(old, new, &lcs);
    for hunk in hunks {
        output.push_str(&hunk);
    }
    output
}

/// Compute LCS table using dynamic programming. Returns a boolean grid
/// indicating which lines from old/new are part of the LCS.
fn compute_lcs(old: &[&str], new: &[&str]) -> Vec<Vec<u32>> {
    let m = old.len();
    let n = new.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    dp
}

/// Build unified diff hunks from old/new lines and LCS table.
fn build_hunks(old: &[&str], new: &[&str], dp: &[Vec<u32>]) -> Vec<String> {
    // First, build the edit operations from the DP table
    let mut ops: Vec<DiffOp> = Vec::new();
    let mut i = old.len();
    let mut j = new.len();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            ops.push(DiffOp::Equal(i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Insert(j - 1));
            j -= 1;
        } else {
            ops.push(DiffOp::Delete(i - 1));
            i -= 1;
        }
    }
    ops.reverse();

    // Group operations into hunks with 3 lines of context
    let context_lines = 3;
    let mut hunks = Vec::new();

    // Find ranges of changes and group them
    let mut change_ranges: Vec<(usize, usize)> = Vec::new();
    let mut range_start: Option<usize> = None;

    for (idx, op) in ops.iter().enumerate() {
        match op {
            DiffOp::Equal(_, _) => {
                if let Some(start) = range_start {
                    change_ranges.push((start, idx - 1));
                    range_start = None;
                }
            }
            _ => {
                if range_start.is_none() {
                    range_start = Some(idx);
                }
            }
        }
    }
    if let Some(start) = range_start {
        change_ranges.push((start, ops.len() - 1));
    }

    // Merge overlapping ranges (with context)
    let mut merged_ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in change_ranges {
        let ctx_start = start.saturating_sub(context_lines);
        let ctx_end = (end + context_lines).min(ops.len() - 1);
        if let Some(last) = merged_ranges.last_mut() {
            if ctx_start <= last.1 + 1 {
                last.1 = ctx_end;
            } else {
                merged_ranges.push((ctx_start, ctx_end));
            }
        } else {
            merged_ranges.push((ctx_start, ctx_end));
        }
    }

    // Generate hunk output for each merged range
    for (range_start, range_end) in merged_ranges {
        let mut old_start = 0usize;
        let mut new_start = 0usize;
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        let mut lines = Vec::new();
        let mut found_first = false;

        for idx in range_start..=range_end {
            if idx >= ops.len() { break; }
            match &ops[idx] {
                DiffOp::Equal(oi, _ni) => {
                    if !found_first {
                        old_start = *oi;
                        // Calculate new_start from old_start
                        new_start = *_ni;
                        found_first = true;
                    }
                    lines.push(format!(" {}", old[*oi]));
                    old_count += 1;
                    new_count += 1;
                }
                DiffOp::Delete(oi) => {
                    if !found_first {
                        old_start = *oi;
                        // Need to find corresponding new line
                        new_start = find_new_pos_at(idx, &ops);
                        found_first = true;
                    }
                    lines.push(format!("-{}", old[*oi]));
                    old_count += 1;
                }
                DiffOp::Insert(ni) => {
                    if !found_first {
                        old_start = find_old_pos_at(idx, &ops);
                        new_start = *ni;
                        found_first = true;
                    }
                    lines.push(format!("+{}", new[*ni]));
                    new_count += 1;
                }
            }
        }

        let mut hunk = format!("@@ -{},{} +{},{} @@\n",
            old_start + 1, old_count, new_start + 1, new_count);
        for line in lines {
            hunk.push_str(&line);
            hunk.push('\n');
        }
        hunks.push(hunk);
    }

    hunks
}

#[derive(Debug)]
enum DiffOp {
    Equal(usize, usize), // old_idx, new_idx
    Delete(usize),       // old_idx
    Insert(usize),       // new_idx
}

fn find_new_pos_at(idx: usize, ops: &[DiffOp]) -> usize {
    // Look backwards for the nearest new-line position
    for i in (0..idx).rev() {
        match &ops[i] {
            DiffOp::Equal(_, ni) => return ni + 1,
            DiffOp::Insert(ni) => return ni + 1,
            _ => {}
        }
    }
    0
}

fn find_old_pos_at(idx: usize, ops: &[DiffOp]) -> usize {
    // Look backwards for the nearest old-line position
    for i in (0..idx).rev() {
        match &ops[i] {
            DiffOp::Equal(oi, _) => return oi + 1,
            DiffOp::Delete(oi) => return oi + 1,
            _ => {}
        }
    }
    0
}

// ── #53 PatchFn ──

pub struct PatchFn;

impl ComputeFunction for PatchFn {
    fn id(&self) -> FunctionId { FunctionId::new("patch", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        // Spec 9.5: apply unified diff to original text
        let original = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("patch requires UTF-8 original".into()))?;
        let diff_text = std::str::from_utf8(&inputs[1])
            .map_err(|_| ComputeError::ExecutionFailed("patch requires UTF-8 diff".into()))?;

        if diff_text.is_empty() {
            return Ok(inputs[0].clone());
        }

        let original_lines: Vec<&str> = original.lines().collect();
        let hunks = parse_unified_diff(diff_text)
            .map_err(|e| ComputeError::ExecutionFailed(format!("invalid unified diff: {}", e)))?;

        // Apply hunks in reverse order to preserve line numbers
        let mut result_lines = original_lines.clone();
        let mut offset: isize = 0;

        for hunk in &hunks {
            let start = ((hunk.old_start as isize - 1) + offset) as usize;
            // Verify context matches
            let mut old_idx = start;
            let mut new_lines_for_hunk: Vec<&str> = Vec::new();
            let mut old_lines_count = 0usize;

            for op in &hunk.ops {
                match op {
                    HunkOp::Context(line) => {
                        if old_idx >= result_lines.len() || result_lines[old_idx] != *line {
                            return Err(ComputeError::ExecutionFailed(
                                format!("diff does not apply cleanly: context mismatch at line {}", old_idx + 1)
                            ));
                        }
                        new_lines_for_hunk.push(line);
                        old_idx += 1;
                        old_lines_count += 1;
                    }
                    HunkOp::Delete(line) => {
                        if old_idx >= result_lines.len() || result_lines[old_idx] != *line {
                            return Err(ComputeError::ExecutionFailed(
                                format!("diff does not apply cleanly: delete mismatch at line {}", old_idx + 1)
                            ));
                        }
                        old_idx += 1;
                        old_lines_count += 1;
                    }
                    HunkOp::Insert(line) => {
                        new_lines_for_hunk.push(line);
                    }
                }
            }

            // Replace the old range with the new lines
            let end = start + old_lines_count;
            result_lines.splice(start..end, new_lines_for_hunk.into_iter().map(|s| s));
            offset += hunk.new_count as isize - hunk.old_count as isize;
        }

        let mut output = result_lines.join("\n");
        // Preserve trailing newline if original had one
        if original.ends_with('\n') && !output.ends_with('\n') {
            output.push('\n');
        }
        Ok(Bytes::from(output))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

struct Hunk<'a> {
    old_start: usize,
    old_count: usize,
    #[allow(dead_code)]
    new_start: usize,
    new_count: usize,
    ops: Vec<HunkOp<'a>>,
}

enum HunkOp<'a> {
    Context(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn parse_unified_diff(diff: &str) -> Result<Vec<Hunk<'_>>, String> {
    let mut hunks = Vec::new();
    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;

    // Skip file headers (--- and +++ lines)
    while i < lines.len() {
        if lines[i].starts_with("@@") {
            break;
        }
        i += 1;
    }

    while i < lines.len() {
        if !lines[i].starts_with("@@") {
            i += 1;
            continue;
        }

        // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
        let header = lines[i];
        let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)?;
        i += 1;

        let mut ops = Vec::new();
        while i < lines.len() && !lines[i].starts_with("@@") {
            let line = lines[i];
            if line.starts_with(' ') {
                ops.push(HunkOp::Context(&line[1..]));
            } else if line.starts_with('-') {
                ops.push(HunkOp::Delete(&line[1..]));
            } else if line.starts_with('+') {
                ops.push(HunkOp::Insert(&line[1..]));
            } else if line.is_empty() {
                // Empty line in diff = context line with empty content
                ops.push(HunkOp::Context(""));
            }
            i += 1;
        }

        hunks.push(Hunk { old_start, old_count, new_start, new_count, ops });
    }

    Ok(hunks)
}

fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), String> {
    // @@ -1,3 +1,4 @@ or @@ -1 +1,2 @@
    let header = header.trim_start_matches("@@ ").trim_end_matches(" @@");
    // Remove trailing context after last @@
    let header = if let Some(pos) = header.rfind(" @@") {
        &header[..pos]
    } else {
        header
    };
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 2 {
        return Err("invalid hunk header".into());
    }

    let old_part = parts[0].trim_start_matches('-');
    let new_part = parts[1].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Ok((old_start, old_count, new_start, new_count))
}

fn parse_range(range: &str) -> Result<(usize, usize), String> {
    if let Some(comma) = range.find(',') {
        let start: usize = range[..comma].parse().map_err(|_| "invalid range start")?;
        let count: usize = range[comma + 1..].parse().map_err(|_| "invalid range count")?;
        Ok((start, count))
    } else {
        let start: usize = range.parse().map_err(|_| "invalid range")?;
        Ok((start, 1))
    }
}

// ── #54 MergeSortedFn ──

pub struct MergeSortedFn;

impl ComputeFunction for MergeSortedFn {
    fn id(&self) -> FunctionId { FunctionId::new("merge_sorted", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.is_empty() { return Err(ComputeError::InputCount { expected: 2, got: 0 }); }
        let strings: Vec<&str> = inputs.iter()
            .map(|i| std::str::from_utf8(i).map_err(|_| ComputeError::ExecutionFailed("merge_sorted requires UTF-8".into())))
            .collect::<Result<_, _>>()?;
        let mut iters: Vec<std::iter::Peekable<std::str::Lines<'_>>> =
            strings.iter().map(|s| s.lines().peekable()).collect();
        let mut lines = Vec::new();
        loop {
            let mut best: Option<(usize, &str)> = None;
            for (i, iter) in iters.iter_mut().enumerate() {
                if let Some(&line) = iter.peek() {
                    if best.is_none() || line < best.unwrap().1 { best = Some((i, line)); }
                }
            }
            match best {
                Some((i, _)) => { lines.push(iters[i].next().unwrap()); }
                None => break,
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── Spec 9.6: MergeFn (registered as "merge@1.0.0") ──

pub struct MergeFn;

impl ComputeFunction for MergeFn {
    fn id(&self) -> FunctionId { FunctionId::new("merge", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let strings: Vec<&str> = inputs.iter()
            .map(|i| std::str::from_utf8(i).map_err(|_| ComputeError::ExecutionFailed("merge requires UTF-8".into())))
            .collect::<Result<_, _>>()?;
        let mut iters: Vec<std::iter::Peekable<std::str::Lines<'_>>> =
            strings.iter().map(|s| s.lines().peekable()).collect();
        let mut lines = Vec::new();
        loop {
            let mut best: Option<(usize, &str)> = None;
            for (i, iter) in iters.iter_mut().enumerate() {
                if let Some(&line) = iter.peek() {
                    if best.is_none() || line < best.unwrap().1 { best = Some((i, line)); }
                }
            }
            match best {
                Some((i, _)) => { lines.push(iters[i].next().unwrap()); }
                None => break,
            }
        }
        Ok(Bytes::from(lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(100, input_sizes) }
}

// ── #55 SelectFn ──

pub struct SelectFn;

impl ComputeFunction for SelectFn {
    fn id(&self) -> FunctionId { FunctionId::new("select", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let idx: usize = parse_usize_param(params, "index")?;
        if idx >= inputs.len() {
            return Err(ComputeError::ExecutionFailed(format!("index {} out of range (have {} inputs)", idx, inputs.len())));
        }
        Ok(inputs[idx].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── Spec 9.7: SelectInputFn (registered as "select_input@1.0.0") ──

pub struct SelectInputFn;

impl ComputeFunction for SelectInputFn {
    fn id(&self) -> FunctionId { FunctionId::new("select_input", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let idx_str = get_string_param(params, "index")?;
        let idx: usize = idx_str.parse().map_err(|_| ComputeError::InvalidParam("index must be a non-negative integer".into()))?;
        if idx >= inputs.len() {
            return Err(ComputeError::ExecutionFailed(format!("index {} out of range (have {} inputs)", idx, inputs.len())));
        }
        Ok(inputs[idx].clone())
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── Spec 9.2: InterleaveBytesFn (registered as "interleave_bytes@1.0.0") ──

pub struct InterleaveBytesFn;

impl ComputeFunction for InterleaveBytesFn {
    fn id(&self) -> FunctionId { FunctionId::new("interleave_bytes", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() < 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        // Byte-by-byte round-robin interleave
        let total: usize = inputs.iter().map(|i| i.len()).sum();
        let max_len = inputs.iter().map(|i| i.len()).max().unwrap_or(0);
        let mut out = Vec::with_capacity(total);
        for byte_idx in 0..max_len {
            for input in &inputs {
                if byte_idx < input.len() {
                    out.push(input[byte_idx]);
                }
            }
        }
        Ok(Bytes::from(out))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── Spec 9.8: AlternateFn (registered as "alternate@1.0.0") ──

pub struct AlternateFn;

impl ComputeFunction for AlternateFn {
    fn id(&self) -> FunctionId { FunctionId::new("alternate", "1.0.0") }
    fn execute(&self, inputs: Vec<Bytes>, _params: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        if inputs.len() != 2 { return Err(ComputeError::InputCount { expected: 2, got: inputs.len() }); }
        let a_str = std::str::from_utf8(&inputs[0])
            .map_err(|_| ComputeError::ExecutionFailed("alternate requires UTF-8 input".into()))?;
        let b_str = std::str::from_utf8(&inputs[1])
            .map_err(|_| ComputeError::ExecutionFailed("alternate requires UTF-8 input".into()))?;

        let a_lines: Vec<&str> = a_str.lines().collect();
        let b_lines: Vec<&str> = b_str.lines().collect();
        let max_len = a_lines.len().max(b_lines.len());

        let mut out_lines: Vec<&str> = Vec::new();
        for i in 0..max_len {
            if let Some(a) = a_lines.get(i) {
                out_lines.push(a);
            }
            if let Some(b) = b_lines.get(i) {
                out_lines.push(b);
            }
        }
        Ok(Bytes::from(out_lines.join("\n")))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost { spec_cost(10, input_sizes) }
}

// ── #56 TakeFn ──

