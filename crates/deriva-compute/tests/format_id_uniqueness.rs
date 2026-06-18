//! ID uniqueness validation
//!
//! Verifies no duplicate function IDs exist across all format-aware functions,
//! and no overlap with core library IDs.

use deriva_compute::builtins_format::register_format_functions;
use deriva_compute::function::ComputeFunction;
use std::collections::HashSet;

#[test]
fn format_function_ids_are_unique() {
    let fns = register_format_functions();
    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    for f in &fns {
        let id = f.id();
        let key = format!("{}/{}", id.name, id.version);
        if !seen.insert(key.clone()) {
            duplicates.push(key);
        }
    }
    assert!(
        duplicates.is_empty(),
        "Duplicate function IDs found: {:?}",
        duplicates
    );
}

#[test]
fn format_functions_do_not_overlap_with_core() {
    // Core builtins from builtins/mod.rs — these are the original 100 functions
    let core_names: &[&str] = &[
        "identity", "concat", "uppercase", "lowercase", "reverse",
        "base64_encode", "base64_decode", "hex_encode", "hex_decode",
        "base32_encode", "base32_decode", "xor", "bitwise_and", "bitwise_or",
        "bitwise_not", "byte_swap", "trim", "pad", "line_ending",
        "compress", "decompress", "zstd_compress", "zstd_decompress",
        "lz4_compress", "lz4_decompress", "snappy_compress", "snappy_decompress",
        "brotli_compress", "brotli_decompress", "sha256", "sha512", "md5",
        "blake3", "hmac_sha256", "crc32", "encrypt", "decrypt",
        "aead_encrypt", "aead_decrypt", "redact",
        "byte_count", "line_count", "word_count", "histogram", "entropy",
        "min_max", "sum", "average", "interleave", "zip_concat",
        "diff", "patch", "merge_sorted", "select",
        "take", "skip", "slice", "sort", "unique", "sort_unique",
        "shuffle", "head", "tail", "sample", "replace", "regex_replace",
        "grep", "grep_invert", "prefix", "suffix", "line_prefix",
        "line_number", "truncate_lines", "charset_convert", "utf8_validate",
        "json_validate", "schema_validate", "magic_bytes", "size_limit",
        "non_empty", "sha256_verify", "crc32_verify",
        "json_pretty_print", "json_minify", "csv_to_json", "json_to_csv",
        "json_lines", "yaml_to_json", "json_to_yaml", "toml_to_json",
        "caddr_compute", "caddr_verify", "caddr_embed", "merkle_root",
        "content_type", "chunk_hash", "dedup_analyze",
        "reverse_byte", "sort_bytes", "repeat",
    ];

    let format_fns = register_format_functions();
    let mut collisions = Vec::new();
    for f in &format_fns {
        let id = f.id();
        if core_names.contains(&id.name.as_str()) {
            collisions.push(format!("{}", id));
        }
    }
    assert!(
        collisions.is_empty(),
        "Format functions collide with core builtin names: {:?}",
        collisions
    );
}

#[test]
fn all_format_functions_have_nonempty_names() {
    let fns = register_format_functions();
    for f in &fns {
        let id = f.id();
        assert!(!id.name.is_empty(), "Function has empty name");
        assert!(!id.version.is_empty(), "Function {} has empty version", id.name);
    }
}

#[test]
fn format_function_count_at_least_283() {
    let fns = register_format_functions();
    assert!(
        fns.len() >= 283,
        "Expected at least 283 format functions, got {}",
        fns.len()
    );
}
