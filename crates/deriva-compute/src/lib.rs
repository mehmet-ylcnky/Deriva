#![allow(clippy::collapsible_if)]
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::single_match)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::manual_strip)]
#![allow(clippy::unnecessary_fallible_conversions)]

pub mod async_executor;
pub mod builtins;
pub mod builtins_streaming;

#[cfg(feature = "format-detect")]
pub mod builtins_format_detect;
#[cfg(feature = "format-csv")]
pub mod builtins_format_csv;
#[cfg(feature = "format-config")]
pub mod builtins_format_config;
#[cfg(feature = "format-archive")]
pub mod builtins_format_archive;
#[cfg(feature = "format-log")]
pub mod builtins_format_log;
#[cfg(feature = "format-cas")]
pub mod builtins_format_cas;
#[cfg(feature = "format-erasure")]
pub mod builtins_format_erasure;
#[cfg(feature = "format-serialization")]
pub mod builtins_format_serialization;
#[cfg(feature = "format-columnar")]
pub mod builtins_format_columnar;
#[cfg(feature = "format-image")]
pub mod builtins_format_image;
#[cfg(feature = "format-document")]
pub mod builtins_format_document;
#[cfg(feature = "format-audio")]
pub mod builtins_format_audio;
#[cfg(feature = "format-geo")]
pub mod builtins_format_geo;
#[cfg(feature = "format-scientific")]
pub mod builtins_format_scientific;
#[cfg(feature = "format-database")]
pub mod builtins_format_database;
#[cfg(feature = "format-ml")]
pub mod builtins_format_ml;
#[cfg(feature = "format-network")]
pub mod builtins_format_network;
#[cfg(feature = "format-bio")]
pub mod builtins_format_bio;
#[cfg(feature = "format-binary")]
pub mod builtins_format_binary;

#[cfg(any(
    feature = "format-detect",
    feature = "format-csv",
    feature = "format-config",
    feature = "format-archive",
    feature = "format-log",
    feature = "format-cas",
    feature = "format-erasure",
    feature = "format-serialization",
    feature = "format-columnar",
    feature = "format-image",
    feature = "format-document",
    feature = "format-audio",
    feature = "format-geo",
    feature = "format-scientific",
    feature = "format-database",
    feature = "format-ml",
    feature = "format-network",
    feature = "format-bio",
    feature = "format-binary"
))]
pub mod builtins_format;

pub mod adaptive;
pub mod cache;
pub mod chunk_cache;
pub mod executor;
pub mod function;
pub mod fusion;
pub mod gc;
pub mod invalidation;
pub mod leaf_store;
pub mod memory_budget;
pub mod metrics;
pub mod pipeline;
pub mod registry;
pub mod streaming;
pub mod streaming_executor;

pub use async_executor::AsyncExecutor;
pub use executor::Executor;
pub use function::{ComputeCost, ComputeError, ComputeFunction};
pub use memory_budget::{
    BudgetedReceiver, BudgetedSender, ChunkGuard, GlobalMemoryController, MemoryController,
};
pub use registry::FunctionRegistry;
