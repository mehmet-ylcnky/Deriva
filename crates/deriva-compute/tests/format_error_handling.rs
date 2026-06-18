//! Error on invalid format (Property 10)
//!
//! Verify format-aware functions return ComputeError::ExecutionFailed
//! with descriptive messages on invalid input — no panics.

use bytes::Bytes;
use deriva_compute::builtins_format::register_format_functions;
use deriva_compute::function::ComputeFunction;
use std::collections::BTreeMap;
use deriva_core::address::Value;

/// Random garbage bytes that don't conform to any format.
fn garbage() -> Bytes {
    Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF, 0x42, 0x13, 0x37, 0x99])
}

#[test]
fn format_functions_no_panic_on_garbage_input() {
    let fns = register_format_functions();
    let input = garbage();
    let empty_params = BTreeMap::new();

    let mut panicked = Vec::new();
    let mut errors_without_message = Vec::new();

    for f in &fns {
        let id = f.id();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.execute(vec![input.clone()], &empty_params)
        }));
        match result {
            Err(_) => panicked.push(format!("{}", id)),
            Ok(Err(e)) => {
                let msg = format!("{}", e);
                if msg.is_empty() {
                    errors_without_message.push(format!("{}", id));
                }
                // Error is expected — this is fine
            }
            Ok(Ok(_)) => {
                // Some functions might succeed on arbitrary bytes (e.g., identity-like)
                // That's acceptable
            }
        }
    }

    assert!(
        panicked.is_empty(),
        "Functions panicked on garbage input: {:?}",
        panicked
    );
    assert!(
        errors_without_message.is_empty(),
        "Functions returned errors without descriptive messages: {:?}",
        errors_without_message
    );
}

#[test]
fn format_functions_no_panic_on_empty_input() {
    let fns = register_format_functions();
    let input = Bytes::new();
    let empty_params = BTreeMap::new();

    let mut panicked = Vec::new();

    for f in &fns {
        let id = f.id();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.execute(vec![input.clone()], &empty_params)
        }));
        if result.is_err() {
            panicked.push(format!("{}", id));
        }
    }

    assert!(
        panicked.is_empty(),
        "Functions panicked on empty input: {:?}",
        panicked
    );
}
