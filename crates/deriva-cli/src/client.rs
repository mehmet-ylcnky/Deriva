pub mod proto {
    tonic::include_proto!("deriva");
}

pub use proto::deriva_client::DerivaClient;

pub fn hex_to_addr(hex: &str) -> Result<Vec<u8>, String> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", hex.len()));
    }
    (0..32)
        .map(|i| {
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("invalid hex at pos {}: {}", i * 2, e))
        })
        .collect()
}

pub fn addr_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn parse_param(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid param '{}', expected key=value", s))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}
