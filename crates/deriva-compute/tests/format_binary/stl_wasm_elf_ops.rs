use bytes::Bytes;
use deriva_compute::builtins_format_binary::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- stl_read (5 tests) ----
#[test]
fn stl_read_ascii() {
    let stl = b"solid test\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid";
    let out = StlReadFn.execute(vec![Bytes::from(&stl[..])], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["format"], "ascii");
}

#[test]
fn stl_read_binary() {
    let mut stl = vec![0u8; 84];
    stl[80..84].copy_from_slice(&1u32.to_le_bytes());
    stl.extend_from_slice(&[0u8; 50]);
    let out = StlReadFn.execute(vec![Bytes::from(stl)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["format"], "binary");
}

#[test]
fn stl_read_invalid() {
    assert!(StlReadFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn stl_read_no_input() {
    assert!(StlReadFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn stl_read_id() {
    assert_eq!(StlReadFn.id().name, "stl_read");
}

// ---- stl_convert (5 tests) ----
#[test]
fn stl_convert_basic() {
    let stl = b"solid test\nendsolid";
    let out = StlConvertFn.execute(vec![Bytes::from(&stl[..])], &p(&[("target", "binary")])).unwrap();
    assert!(out.len() > 0);
}

#[test]
fn stl_convert_passthrough() {
    let stl = b"solid test\nendsolid";
    let out = StlConvertFn.execute(vec![Bytes::from(&stl[..])], &p(&[])).unwrap();
    assert_eq!(out.as_ref(), &stl[..]);
}

#[test]
fn stl_convert_empty() {
    let out = StlConvertFn.execute(vec![Bytes::from("")], &p(&[])).unwrap();
    assert_eq!(out.len(), 0);
}

#[test]
fn stl_convert_no_input() {
    assert!(StlConvertFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn stl_convert_id() {
    assert_eq!(StlConvertFn.id().name, "stl_convert");
}

// ---- wasm_validate (5 tests) ----
#[test]
fn wasm_validate_basic() {
    let mut wasm = vec![0u8; 8];
    wasm[..4].copy_from_slice(b"\0asm");
    wasm[4..8].copy_from_slice(&1u32.to_le_bytes());
    let out = WasmValidateFn.execute(vec![Bytes::from(wasm)], &p(&[])).unwrap();
    assert_eq!(&out[..4], b"\0asm");
}

#[test]
fn wasm_validate_invalid() {
    assert!(WasmValidateFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn wasm_validate_no_input() {
    assert!(WasmValidateFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn wasm_validate_passthrough() {
    let mut wasm = vec![0u8; 10];
    wasm[..4].copy_from_slice(b"\0asm");
    wasm[4..8].copy_from_slice(&1u32.to_le_bytes());
    let out = WasmValidateFn.execute(vec![Bytes::from(wasm.clone())], &p(&[])).unwrap();
    assert_eq!(out.as_ref(), &wasm[..]);
}

#[test]
fn wasm_validate_id() {
    assert_eq!(WasmValidateFn.id().name, "wasm_validate");
}

// ---- wasm_metadata (5 tests) ----
#[test]
fn wasm_metadata_basic() {
    let mut wasm = vec![0u8; 8];
    wasm[..4].copy_from_slice(b"\0asm");
    wasm[4..8].copy_from_slice(&1u32.to_le_bytes());
    let out = WasmMetadataFn.execute(vec![Bytes::from(wasm)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["version"], 1);
}

#[test]
fn wasm_metadata_invalid() {
    assert!(WasmMetadataFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn wasm_metadata_no_input() {
    assert!(WasmMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn wasm_metadata_has_size() {
    let mut wasm = vec![0u8; 20];
    wasm[..4].copy_from_slice(b"\0asm");
    wasm[4..8].copy_from_slice(&1u32.to_le_bytes());
    let out = WasmMetadataFn.execute(vec![Bytes::from(wasm)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["size"], 20);
}

#[test]
fn wasm_metadata_id() {
    assert_eq!(WasmMetadataFn.id().name, "wasm_metadata");
}

// ---- elf_metadata (5 tests) ----
#[test]
fn elf_metadata_basic() {
    let mut elf = vec![0u8; 16];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little-endian
    let out = ElfMetadataFn.execute(vec![Bytes::from(elf)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["class"], "64-bit");
    assert_eq!(json["endianness"], "little");
}

#[test]
fn elf_metadata_32bit() {
    let mut elf = vec![0u8; 16];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 1; // 32-bit
    elf[5] = 2; // big-endian
    let out = ElfMetadataFn.execute(vec![Bytes::from(elf)], &p(&[])).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["class"], "32-bit");
}

#[test]
fn elf_metadata_invalid() {
    assert!(ElfMetadataFn.execute(vec![Bytes::from("bad")], &p(&[])).is_err());
}

#[test]
fn elf_metadata_no_input() {
    assert!(ElfMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn elf_metadata_id() {
    assert_eq!(ElfMetadataFn.id().name, "elf_metadata");
}
