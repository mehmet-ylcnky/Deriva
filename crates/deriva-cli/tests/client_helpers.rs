use deriva_cli::client::{addr_to_hex, hex_to_addr, parse_param};

// --- hex_to_addr ---

#[test]
fn valid_hex_roundtrip() {
    let bytes: Vec<u8> = (0..32).collect();
    let hex = addr_to_hex(&bytes);
    let parsed = hex_to_addr(&hex).unwrap();
    assert_eq!(parsed, bytes);
}

#[test]
fn hex_all_zeros() {
    let hex = "0".repeat(64);
    let bytes = hex_to_addr(&hex).unwrap();
    assert_eq!(bytes, vec![0u8; 32]);
}

#[test]
fn hex_all_ff() {
    let hex = "f".repeat(64);
    let bytes = hex_to_addr(&hex).unwrap();
    assert_eq!(bytes, vec![0xFF; 32]);
}

#[test]
fn hex_too_short() {
    assert!(hex_to_addr("abcd").is_err());
}

#[test]
fn hex_too_long() {
    let hex = "a".repeat(66);
    assert!(hex_to_addr(&hex).is_err());
}

#[test]
fn hex_invalid_chars() {
    let mut hex = "a".repeat(62);
    hex.push_str("zz");
    assert!(hex_to_addr(&hex).is_err());
}

#[test]
fn hex_with_whitespace_trimmed() {
    let hex = format!("  {}  ", "ab".repeat(32));
    let result = hex_to_addr(&hex).unwrap();
    assert_eq!(result, vec![0xAB; 32]);
}

// --- addr_to_hex ---

#[test]
fn addr_to_hex_format() {
    assert_eq!(addr_to_hex(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
}

#[test]
fn addr_to_hex_leading_zeros() {
    assert_eq!(addr_to_hex(&[0x00, 0x01, 0x0A]), "00010a");
}

#[test]
fn addr_to_hex_empty() {
    assert_eq!(addr_to_hex(&[]), "");
}

// --- parse_param ---

#[test]
fn parse_simple_param() {
    let (k, v) = parse_param("count=3").unwrap();
    assert_eq!(k, "count");
    assert_eq!(v, "3");
}

#[test]
fn parse_param_with_equals_in_value() {
    let (k, v) = parse_param("expr=a=b").unwrap();
    assert_eq!(k, "expr");
    assert_eq!(v, "a=b");
}

#[test]
fn parse_param_empty_value() {
    let (k, v) = parse_param("flag=").unwrap();
    assert_eq!(k, "flag");
    assert_eq!(v, "");
}

#[test]
fn parse_param_no_equals() {
    assert!(parse_param("noequals").is_err());
}
