use bytes::Bytes;
use deriva_compute::builtins_format_bio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- fasta_parse (5 tests) ----
#[test]
fn fasta_parse_basic() {
    let fasta = ">seq1\nACGT\n>seq2\nTGCA\n";
    let out = FastaParseFn.execute(vec![Bytes::from(fasta)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"id\":\"seq1\""));
    assert!(text.contains("\"sequence\":\"ACGT\""));
}

#[test]
fn fasta_parse_multiline() {
    let fasta = ">seq1\nAC\nGT\n";
    let out = FastaParseFn.execute(vec![Bytes::from(fasta)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"sequence\":\"ACGT\""));
}

#[test]
fn fasta_parse_empty() {
    let out = FastaParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn fasta_parse_no_input() {
    assert!(FastaParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn fasta_parse_id() {
    assert_eq!(FastaParseFn.id().name, "fasta_parse");
}

// ---- fastq_parse (5 tests) ----
#[test]
fn fastq_parse_basic() {
    let fastq = "@seq1\nACGT\n+\nIIII\n";
    let out = FastqParseFn.execute(vec![Bytes::from(fastq)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"id\":\"seq1\""));
    assert!(text.contains("\"quality\":\"IIII\""));
}

#[test]
fn fastq_parse_multiple() {
    let fastq = "@seq1\nACGT\n+\nIIII\n@seq2\nTGCA\n+\nJJJJ\n";
    let out = FastqParseFn.execute(vec![Bytes::from(fastq)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn fastq_parse_empty() {
    let out = FastqParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn fastq_parse_no_input() {
    assert!(FastqParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn fastq_parse_id() {
    assert_eq!(FastqParseFn.id().name, "fastq_parse");
}

// ---- fastq_trim_quality (5 tests) ----
#[test]
fn fastq_trim_quality_basic() {
    let fastq = "@seq1\nACGTN\n+\nIIII!\n";
    let out = FastqTrimQualityFn.execute(vec![Bytes::from(fastq)], &p(&[("min_quality", "20")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("ACGT"));
}

#[test]
fn fastq_trim_quality_no_trim() {
    let fastq = "@seq1\nACGT\n+\nIIII\n";
    let out = FastqTrimQualityFn.execute(vec![Bytes::from(fastq)], &p(&[("min_quality", "20")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("ACGT"));
}

#[test]
fn fastq_trim_quality_empty() {
    let out = FastqTrimQualityFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn fastq_trim_quality_no_input() {
    assert!(FastqTrimQualityFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn fastq_trim_quality_id() {
    assert_eq!(FastqTrimQualityFn.id().name, "fastq_trim_quality");
}

// ---- fastq_filter (5 tests) ----
#[test]
fn fastq_filter_min_length() {
    let fastq = "@seq1\nAC\n+\nII\n@seq2\nACGT\n+\nIIII\n";
    let out = FastqFilterFn.execute(vec![Bytes::from(fastq)], &p(&[("min_length", "3")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("seq2"));
    assert!(!text.contains("seq1"));
}

#[test]
fn fastq_filter_no_filter() {
    let fastq = "@seq1\nACGT\n+\nIIII\n";
    let out = FastqFilterFn.execute(vec![Bytes::from(fastq)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("seq1"));
}

#[test]
fn fastq_filter_empty() {
    let out = FastqFilterFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn fastq_filter_no_input() {
    assert!(FastqFilterFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn fastq_filter_id() {
    assert_eq!(FastqFilterFn.id().name, "fastq_filter");
}

// ---- fasta_to_fastq (5 tests) ----
#[test]
fn fasta_to_fastq_basic() {
    let fasta = ">seq1\nACGT\n";
    let out = FastaToFastqFn.execute(vec![Bytes::from(fasta)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("@seq1"));
    assert!(text.contains("IIII"));
}

#[test]
fn fasta_to_fastq_custom_quality() {
    let fasta = ">seq1\nAC\n";
    let out = FastaToFastqFn.execute(vec![Bytes::from(fasta)], &p(&[("default_quality", "J")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("JJ"));
}

#[test]
fn fasta_to_fastq_empty() {
    let out = FastaToFastqFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn fasta_to_fastq_no_input() {
    assert!(FastaToFastqFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn fasta_to_fastq_id() {
    assert_eq!(FastaToFastqFn.id().name, "fasta_to_fastq");
}
