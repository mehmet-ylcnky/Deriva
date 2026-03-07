use bytes::Bytes;
use deriva_compute::builtins_format_bio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- sam_parse (5 tests) ----
#[test]
fn sam_parse_basic() {
    let sam = "read1\t0\tchr1\t100\t60\t4M\t*\t0\t0\tACGT\tIIII\n";
    let out = SamParseFn.execute(vec![Bytes::from(sam)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"qname\":\"read1\""));
    assert!(text.contains("\"rname\":\"chr1\""));
}

#[test]
fn sam_parse_skip_header() {
    let sam = "@HD\tVN:1.0\nread1\t0\tchr1\t100\t60\t4M\t*\t0\t0\tACGT\tIIII\n";
    let out = SamParseFn.execute(vec![Bytes::from(sam)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn sam_parse_empty() {
    let out = SamParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn sam_parse_no_input() {
    assert!(SamParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn sam_parse_id() {
    assert_eq!(SamParseFn.id().name, "sam_parse");
}

// ---- vcf_parse (5 tests) ----
#[test]
fn vcf_parse_basic() {
    let vcf = "chr1\t100\trs123\tA\tT\t30\tPASS\tDP=10\n";
    let out = VcfParseFn.execute(vec![Bytes::from(vcf)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"chrom\":\"chr1\""));
    assert!(text.contains("\"ref\":\"A\""));
}

#[test]
fn vcf_parse_skip_header() {
    let vcf = "##fileformat=VCFv4.2\n#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\nchr1\t100\t.\tA\tT\t30\tPASS\tDP=10\n";
    let out = VcfParseFn.execute(vec![Bytes::from(vcf)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn vcf_parse_empty() {
    let out = VcfParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn vcf_parse_no_input() {
    assert!(VcfParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn vcf_parse_id() {
    assert_eq!(VcfParseFn.id().name, "vcf_parse");
}

// ---- vcf_filter (5 tests) ----
#[test]
fn vcf_filter_by_chrom() {
    let vcf = "##fileformat=VCFv4.2\nchr1\t100\t.\tA\tT\t30\tPASS\tDP=10\nchr2\t200\t.\tC\tG\t40\tPASS\tDP=20\n";
    let out = VcfFilterFn.execute(vec![Bytes::from(vcf)], &p(&[("chrom", "chr1")])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("chr1"));
    assert!(!text.contains("chr2"));
}

#[test]
fn vcf_filter_preserve_header() {
    let vcf = "##fileformat=VCFv4.2\nchr1\t100\t.\tA\tT\t30\tPASS\tDP=10\n";
    let out = VcfFilterFn.execute(vec![Bytes::from(vcf)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("##fileformat"));
}

#[test]
fn vcf_filter_empty() {
    let out = VcfFilterFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn vcf_filter_no_input() {
    assert!(VcfFilterFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn vcf_filter_id() {
    assert_eq!(VcfFilterFn.id().name, "vcf_filter");
}

// ---- bed_parse (5 tests) ----
#[test]
fn bed_parse_basic() {
    let bed = "chr1\t100\t200\n";
    let out = BedParseFn.execute(vec![Bytes::from(bed)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"chrom\":\"chr1\""));
    assert!(text.contains("\"start\":100"));
}

#[test]
fn bed_parse_with_name() {
    let bed = "chr1\t100\t200\tfeature1\t50\t+\n";
    let out = BedParseFn.execute(vec![Bytes::from(bed)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"name\":\"feature1\""));
}

#[test]
fn bed_parse_empty() {
    let out = BedParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn bed_parse_no_input() {
    assert!(BedParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn bed_parse_id() {
    assert_eq!(BedParseFn.id().name, "bed_parse");
}

// ---- gff_parse (5 tests) ----
#[test]
fn gff_parse_basic() {
    let gff = "chr1\ttest\tgene\t100\t200\t.\t+\t.\tID=gene1\n";
    let out = GffParseFn.execute(vec![Bytes::from(gff)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    assert!(text.contains("\"seqid\":\"chr1\""));
    assert!(text.contains("\"type\":\"gene\""));
}

#[test]
fn gff_parse_skip_comments() {
    let gff = "#comment\nchr1\ttest\tgene\t100\t200\t.\t+\t.\tID=gene1\n";
    let out = GffParseFn.execute(vec![Bytes::from(gff)], &p(&[])).unwrap();
    let text = std::str::from_utf8(&out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
}

#[test]
fn gff_parse_empty() {
    let out = GffParseFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn gff_parse_no_input() {
    assert!(GffParseFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn gff_parse_id() {
    assert_eq!(GffParseFn.id().name, "gff_parse");
}
