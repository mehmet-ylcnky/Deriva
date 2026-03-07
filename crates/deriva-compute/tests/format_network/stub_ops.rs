use bytes::Bytes;
use deriva_compute::builtins_format_network::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- pcap_read (5 tests) ----
#[test]
fn pcap_read_requires_lib() { assert!(PcapReadFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn pcap_read_id() { assert_eq!(PcapReadFn.id().name, "pcap_read"); }
#[test]
fn pcap_read_no_input() { assert!(PcapReadFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn pcap_read_with_data() { assert!(PcapReadFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn pcap_read_error_msg() {
    let err = PcapReadFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("pcap-parser"));
}

// ---- pcap_filter (5 tests) ----
#[test]
fn pcap_filter_requires_lib() { assert!(PcapFilterFn.execute(vec![Bytes::new()], &p(&[("filter", "tcp port 80")])).is_err()); }
#[test]
fn pcap_filter_id() { assert_eq!(PcapFilterFn.id().name, "pcap_filter"); }
#[test]
fn pcap_filter_no_input() { assert!(PcapFilterFn.execute(vec![], &p(&[("filter", "tcp")])).is_err()); }
#[test]
fn pcap_filter_with_filter() { assert!(PcapFilterFn.execute(vec![Bytes::new()], &p(&[("filter", "udp")])).is_err()); }
#[test]
fn pcap_filter_error_msg() {
    let err = PcapFilterFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-network-pcap"));
}

// ---- pcap_statistics (5 tests) ----
#[test]
fn pcap_statistics_requires_lib() { assert!(PcapStatisticsFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn pcap_statistics_id() { assert_eq!(PcapStatisticsFn.id().name, "pcap_statistics"); }
#[test]
fn pcap_statistics_no_input() { assert!(PcapStatisticsFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn pcap_statistics_with_data() { assert!(PcapStatisticsFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn pcap_statistics_error_msg() {
    let err = PcapStatisticsFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("pcap-parser"));
}

// ---- email_parse (5 tests) ----
#[test]
fn email_parse_requires_lib() { assert!(EmailParseFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn email_parse_id() { assert_eq!(EmailParseFn.id().name, "email_parse"); }
#[test]
fn email_parse_no_input() { assert!(EmailParseFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn email_parse_with_data() { assert!(EmailParseFn.execute(vec![Bytes::from_static(b"x")], &p(&[])).is_err()); }
#[test]
fn email_parse_error_msg() {
    let err = EmailParseFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("mailparse"));
}

// ---- email_extract_attachments (5 tests) ----
#[test]
fn email_extract_attachments_requires_lib() { assert!(EmailExtractAttachmentsFn.execute(vec![Bytes::new()], &p(&[])).is_err()); }
#[test]
fn email_extract_attachments_id() { assert_eq!(EmailExtractAttachmentsFn.id().name, "email_extract_attachments"); }
#[test]
fn email_extract_attachments_no_input() { assert!(EmailExtractAttachmentsFn.execute(vec![], &p(&[])).is_err()); }
#[test]
fn email_extract_attachments_with_index() { assert!(EmailExtractAttachmentsFn.execute(vec![Bytes::new()], &p(&[("index", "0")])).is_err()); }
#[test]
fn email_extract_attachments_error_msg() {
    let err = EmailExtractAttachmentsFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-network-email"));
}
