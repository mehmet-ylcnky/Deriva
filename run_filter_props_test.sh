#!/bin/bash
cargo test -p deriva-compute --features format-columnar --test format_columnar_filter_props -- --test-threads=1 2>&1
