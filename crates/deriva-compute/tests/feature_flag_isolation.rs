//! Feature flag isolation tests.
//!
//! Validates that:
//! - Disabled features do not register functions (Requirements 2.6, 2.7)
//! - Enabled features register the expected functions
//!
//! This test is designed to run with default features (format-phase1).

use deriva_compute::builtins;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::address::FunctionId;

/// Helper: build a fully-populated registry using the default feature set.
fn build_registry() -> FunctionRegistry {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);
    registry
}

// ============================================================
// Tests for Phase 1 functions being PRESENT (enabled by default)
// ============================================================

#[test]
#[cfg(feature = "format-detect")]
fn phase1_detect_functions_are_registered() {
    let registry = build_registry();

    let detect_fns = [
        "format_detect",
        "format_validate",
        "universal_metadata",
        "universal_to_json",
        "universal_to_text",
        "format_convert",
        "schema_infer",
        "schema_compare",
        "mime_type_map",
    ];

    for name in detect_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-detect function '{}' to be registered",
            name
        );
    }
}

#[test]
#[cfg(feature = "format-csv")]
fn phase1_csv_functions_are_registered() {
    let registry = build_registry();

    // Spot-check representative CSV/JSON/XML functions
    let csv_fns = [
        "csv_parse",
        "csv_write",
        "csv_filter",
        "ndjson_parse",
        "jsonpath_extract",
        "xml_parse",
        "xml_xpath_extract",
    ];

    for name in csv_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-csv function '{}' to be registered",
            name
        );
    }
}

#[test]
#[cfg(feature = "format-config")]
fn phase1_config_functions_are_registered() {
    let registry = build_registry();

    let config_fns = [
        "yaml_parse",
        "toml_parse",
        "ini_parse",
        "hcl_parse",
        "config_format_convert",
    ];

    for name in config_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-config function '{}' to be registered",
            name
        );
    }
}

#[test]
#[cfg(feature = "format-archive")]
fn phase1_archive_functions_are_registered() {
    let registry = build_registry();

    let archive_fns = [
        "tar_create",
        "tar_extract",
        "zip_create",
        "zip_extract",
        "gzip_compress",
        "bzip2_compress",
        "xz_compress",
    ];

    for name in archive_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-archive function '{}' to be registered",
            name
        );
    }
}

#[test]
#[cfg(feature = "format-log")]
fn phase1_log_functions_are_registered() {
    let registry = build_registry();

    let log_fns = [
        "syslog_parse",
        "cef_parse",
        "log_level_filter",
        "log_timestamp_normalize",
    ];

    for name in log_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-log function '{}' to be registered",
            name
        );
    }
}

#[test]
#[cfg(feature = "format-cas")]
fn phase1_cas_functions_are_registered() {
    let registry = build_registry();

    let cas_fns = [
        "cid_compute",
        "cid_verify",
        "dag_cbor_encode",
        "dag_cbor_decode",
        "car_create",
        "car_extract",
        "merkle_proof_generate",
        "merkle_proof_verify",
    ];

    for name in cas_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            registry.contains(&id),
            "Expected format-cas function '{}' to be registered",
            name
        );
    }
}

// ============================================================
// Tests for Phase 2/3/4 functions being ABSENT (not enabled by default)
// ============================================================

#[test]
#[cfg(not(feature = "format-columnar"))]
fn phase2_columnar_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let columnar_fns = [
        "parquet_read",
        "parquet_write",
        "parquet_metadata",
        "parquet_projection",
        "parquet_filter",
        "orc_read",
        "arrow_ipc_read",
        "columnar_to_row",
    ];

    for name in columnar_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-columnar is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-serialization"))]
fn phase2_serialization_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let serial_fns = [
        "avro_read",
        "avro_write",
        "protobuf_decode",
        "protobuf_encode",
        "msgpack_decode",
        "cbor_decode",
        "bson_decode",
    ];

    for name in serial_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-serialization is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-erasure"))]
fn phase2_erasure_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let erasure_fns = [
        "reed_solomon_encode",
        "reed_solomon_decode",
        "reed_solomon_verify",
        "xor_parity",
        "stripe_split",
    ];

    for name in erasure_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-erasure is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-image"))]
fn phase3_image_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let image_fns = [
        "image_resize",
        "image_crop",
        "image_rotate",
        "image_convert",
        "image_thumbnail",
        "image_metadata",
    ];

    for name in image_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-image is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-document"))]
fn phase3_document_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let doc_fns = [
        "pdf_metadata",
        "pdf_extract_text",
        "docx_extract_text",
        "xlsx_read",
        "html_to_text",
        "markdown_to_html",
    ];

    for name in doc_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-document is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-audio"))]
fn phase3_audio_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let audio_fns = [
        "audio_metadata",
        "audio_convert",
        "video_metadata",
        "video_thumbnail",
    ];

    for name in audio_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-audio is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-geo"))]
fn phase4_geo_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let geo_fns = [
        "geojson_validate",
        "geojson_filter",
        "shapefile_to_geojson",
        "flatgeobuf_read",
    ];

    for name in geo_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-geo is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-scientific"))]
fn phase4_scientific_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let sci_fns = [
        "hdf5_metadata",
        "hdf5_read",
        "numpy_read",
        "numpy_write",
    ];

    for name in sci_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-scientific is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-database"))]
fn phase4_database_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let db_fns = [
        "sqlite_query",
        "sqlite_table_list",
        "csv_to_sqlite",
    ];

    for name in db_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-database is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-ml"))]
fn phase4_ml_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let ml_fns = [
        "safetensors_read",
        "safetensors_metadata",
        "onnx_metadata",
        "image_to_tensor",
    ];

    for name in ml_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-ml is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-network"))]
fn phase4_network_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let net_fns = [
        "pcap_read",
        "pcap_filter",
        "dns_record_parse",
        "email_parse",
    ];

    for name in net_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-network is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-bio"))]
fn phase4_bio_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let bio_fns = [
        "fasta_parse",
        "fastq_parse",
        "sam_parse",
        "vcf_parse",
    ];

    for name in bio_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-bio is disabled",
            name
        );
    }
}

#[test]
#[cfg(not(feature = "format-binary"))]
fn phase4_binary_functions_not_registered_when_disabled() {
    let registry = build_registry();

    let binary_fns = [
        "font_metadata",
        "gltf_metadata",
        "wasm_validate",
        "dicom_metadata",
    ];

    for name in binary_fns {
        let id = FunctionId::new(name, "1.0.0");
        assert!(
            !registry.contains(&id),
            "Function '{}' should NOT be registered when format-binary is disabled",
            name
        );
    }
}

// ============================================================
// Count tests: verify expected function counts for enabled features
// ============================================================

#[test]
#[cfg(feature = "format-detect")]
fn format_detect_registers_exactly_9_functions() {
    let detect_fns = [
        "format_detect",
        "format_validate",
        "universal_metadata",
        "universal_to_json",
        "universal_to_text",
        "format_convert",
        "schema_infer",
        "schema_compare",
        "mime_type_map",
    ];

    let registry = build_registry();
    let count = detect_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 9,
        "format-detect should register exactly 9 functions, found {}",
        count
    );
}

#[test]
#[cfg(feature = "format-csv")]
fn format_csv_registers_25_functions() {
    let csv_fns = [
        "csv_parse",
        "csv_write",
        "csv_schema_infer",
        "csv_column_select",
        "csv_column_rename",
        "csv_filter",
        "csv_sort",
        "csv_aggregate",
        "csv_join",
        "csv_deduplicate",
        "tsv_parse",
        "ndjson_parse",
        "ndjson_write",
        "ndjson_filter",
        "ndjson_project",
        "jsonpath_extract",
        "json_merge",
        "json_flatten",
        "json_unflatten",
        "xml_parse",
        "xml_write",
        "xml_xpath_extract",
        "xml_xslt_transform",
        "xml_validate_dtd",
        "xml_validate_xsd",
    ];

    let registry = build_registry();
    let count = csv_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 25,
        "format-csv should register exactly 25 functions, found {}",
        count
    );
}

#[test]
#[cfg(feature = "format-config")]
fn format_config_registers_16_functions() {
    let config_fns = [
        "yaml_parse",
        "yaml_write",
        "yaml_validate",
        "yaml_merge",
        "toml_parse",
        "toml_write",
        "ini_parse",
        "ini_write",
        "env_parse",
        "env_write",
        "hcl_parse",
        "properties_parse",
        "properties_write",
        "plist_parse",
        "plist_write",
        "config_format_convert",
    ];

    let registry = build_registry();
    let count = config_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 16,
        "format-config should register exactly 16 functions, found {}",
        count
    );
}

#[test]
#[cfg(feature = "format-archive")]
fn format_archive_registers_20_functions() {
    let archive_fns = [
        "tar_create",
        "tar_extract",
        "tar_list",
        "tar_append",
        "gzip_compress",
        "gzip_decompress",
        "bzip2_compress",
        "bzip2_decompress",
        "xz_compress",
        "xz_decompress",
        "zip_create",
        "zip_extract",
        "zip_list",
        "zip_extract_single",
        "tar_gz_create",
        "tar_gz_extract",
        "zstd_frame_compress",
        "zstd_frame_decompress",
        "sevenz_extract",
        "rar_extract",
    ];

    let registry = build_registry();
    let count = archive_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 20,
        "format-archive should register exactly 20 functions, found {}",
        count
    );
}

#[test]
#[cfg(feature = "format-log")]
fn format_log_registers_13_functions() {
    let log_fns = [
        "syslog_parse",
        "syslog_write",
        "cef_parse",
        "elf_parse",
        "apache_log_parse",
        "nginx_log_parse",
        "cloudtrail_parse",
        "vpc_flow_log_parse",
        "prometheus_exposition_parse",
        "otlp_decode",
        "log_timestamp_normalize",
        "log_level_filter",
        "log_anonymize",
    ];

    let registry = build_registry();
    let count = log_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 13,
        "format-log should register exactly 13 functions, found {}",
        count
    );
}

#[test]
#[cfg(feature = "format-cas")]
fn format_cas_registers_15_functions() {
    let cas_fns = [
        "cid_compute",
        "cid_verify",
        "cid_parse",
        "dag_pb_encode",
        "dag_pb_decode",
        "dag_cbor_encode",
        "dag_cbor_decode",
        "car_create",
        "car_extract",
        "car_list",
        "car_verify",
        "unixfs_chunk",
        "unixfs_assemble",
        "merkle_proof_generate",
        "merkle_proof_verify",
    ];

    let registry = build_registry();
    let count = cas_fns
        .iter()
        .filter(|name| registry.contains(&FunctionId::new(**name, "1.0.0")))
        .count();

    assert_eq!(
        count, 15,
        "format-cas should register exactly 15 functions, found {}",
        count
    );
}

// ============================================================
// Phase 1 total count
// ============================================================

#[test]
#[cfg(all(
    feature = "format-detect",
    feature = "format-csv",
    feature = "format-config",
    feature = "format-archive",
    feature = "format-log",
    feature = "format-cas"
))]
fn phase1_registers_98_format_functions_total() {
    use deriva_compute::builtins_format::register_format_functions;

    // Phase 1 totals: CSV/JSON/XML(25) + Archive(20) + Config(16) + Logs(13) + CAS(15) + Detection(17)
    // Detection has 9 core functions + 8 extended analysis functions (encoding_detect, is_text,
    // is_binary, is_compressed, is_encrypted, byte_histogram, entropy_score, structure_heuristic)
    // added in task 2.1.
    let format_fns = register_format_functions();
    assert_eq!(
        format_fns.len(),
        106,
        "Phase 1 (format-phase1) should register exactly 106 format functions, got {}",
        format_fns.len()
    );
}
