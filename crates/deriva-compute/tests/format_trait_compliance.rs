//! Trait compliance (Property 1) and Feature isolation (Property 2)
//!
//! Validates that format-aware functions properly implement ComputeFunction trait
//! and that feature flags correctly control function availability.

use bytes::Bytes;
use deriva_compute::builtins;
use deriva_compute::builtins_format::register_format_functions;
use deriva_compute::function::ComputeFunction;
use deriva_compute::registry::FunctionRegistry;
use std::collections::BTreeMap;
use deriva_core::address::{FunctionId, Value};

/// Property 1: All format functions implement ComputeFunction correctly.
/// - id() returns a valid FunctionId with non-empty name and version
/// - estimated_cost() doesn't panic
/// - execute() with valid single-byte input doesn't panic (may error)
#[test]
fn trait_compliance_all_format_functions() {
    let fns = register_format_functions();

    for f in &fns {
        // id() returns valid
        let id = f.id();
        assert!(!id.name.is_empty());
        assert!(!id.version.is_empty());

        // estimated_cost() doesn't panic
        let _ = f.estimated_cost(&[100]);
        let _ = f.estimated_cost(&[]);
        let _ = f.estimated_cost(&[0, 1024, 65536]);

        // execute() with minimal input doesn't panic (errors are fine)
        let _ = f.execute(vec![Bytes::from_static(b"x")], &BTreeMap::new());
    }
}

/// Property 2: Feature isolation — enabled features register their functions.
#[test]
fn feature_isolation_registry_consistency() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    // Core functions should always be present
    let core_identity = FunctionId::new("identity", "1.0.0");
    assert!(registry.contains(&core_identity), "Core identity function missing");

    // Format functions registered via register_all should be findable
    let format_fns = register_format_functions();
    for f in &format_fns {
        let id = f.id();
        assert!(
            registry.contains(&id),
            "Format function {} missing from registry after register_all",
            id
        );
    }
}

/// Verify format functions don't shadow core builtins
#[test]
fn no_format_core_shadowing() {
    let mut registry = FunctionRegistry::new();
    builtins::register_all(&mut registry);

    let core_names = ["identity", "concat", "uppercase", "lowercase", "reverse",
        "base64_encode", "base64_decode", "sha256", "compress", "decompress"];

    for name in &core_names {
        let id = FunctionId::new(*name, "1.0.0");
        assert!(registry.contains(&id), "Core function {} should be in registry", name);
    }

    // Format functions shouldn't use these exact names
    let format_fns = register_format_functions();
    for f in &format_fns {
        let id = f.id();
        assert!(
            !core_names.contains(&id.name.as_str()) || id.version != "1.0.0",
            "Format function {} shadows core builtin",
            id
        );
    }
}
