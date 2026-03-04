#![cfg(feature = "extended-streaming")]

mod streaming_ext {
    mod crypto;
    mod encoding;
    mod analytics;
    mod flow;
    mod validation;
    mod text;
    mod cas;
    mod compression;
    mod numeric;
    mod integration;
    mod spec_gaps;
    mod edge_cases;
}
