//! Adaptive chunk sizing for streaming pipelines (§2.10).
//!
//! This module provides per-stage dynamic chunk sizing driven by throughput
//! feedback. Each `AdaptiveResizer` node measures the producer/consumer
//! throughput ratio using exponential moving averages (EMA) and adjusts chunk
//! boundaries using configurable growth/shrink factors with hysteresis to
//! prevent oscillation.

use std::time::Instant;

use bytes::BytesMut;
use tokio::sync::mpsc;

use crate::memory_budget::MemoryController;
use crate::metrics::{ADAPTIVE_BUDGET_SUPPRESS_TOTAL, ADAPTIVE_CHUNK_SIZE_BYTES, ADAPTIVE_RESIZE_EVENTS_TOTAL};
use deriva_core::streaming::StreamChunk;

/// Minimum chunk size the AdaptiveResizer can shrink to (1 KB).
pub const MIN_CHUNK_SIZE: usize = 1024;

/// Maximum chunk size the AdaptiveResizer can grow to (1 MB).
pub const MAX_CHUNK_SIZE: usize = 1_048_576;

/// Default EMA smoothing factor (α).
pub const DEFAULT_EMA_ALPHA: f64 = 0.3;

/// Default threshold above which the resizer grows chunk size.
pub const DEFAULT_GROW_RATIO: f64 = 1.5;

/// Default threshold below which the resizer shrinks chunk size.
pub const DEFAULT_SHRINK_RATIO: f64 = 0.7;

/// Default multiplier applied when growing chunk size.
pub const DEFAULT_GROWTH_FACTOR: f64 = 2.0;

/// Default multiplier applied when shrinking chunk size.
pub const DEFAULT_SHRINK_FACTOR: f64 = 0.5;

/// Per-stage configuration for adaptive chunk sizing.
///
/// Controls the EMA smoothing, growth/shrink behavior, and ratio thresholds
/// for an individual `AdaptiveResizer` node.
#[derive(Debug, Clone)]
pub struct AdaptiveChunkConfig {
    /// EMA smoothing factor (α). Must be in the exclusive range (0.0, 1.0).
    /// Higher values react faster to throughput changes but are noisier.
    pub smoothing_factor: f64,

    /// Multiplier applied to chunk size when growing. Must be > 1.0.
    pub growth_factor: f64,

    /// Multiplier applied to chunk size when shrinking. Must be in (0.0, 1.0).
    pub shrink_factor: f64,

    /// Reserved for future latency-based adaptation. Currently unused.
    pub target_latency_us: u64,

    /// Throughput ratio threshold above which the resizer grows chunk size.
    pub grow_ratio: f64,

    /// Throughput ratio threshold below which the resizer shrinks chunk size.
    pub shrink_ratio: f64,
}

impl Default for AdaptiveChunkConfig {
    fn default() -> Self {
        Self {
            smoothing_factor: DEFAULT_EMA_ALPHA,
            growth_factor: DEFAULT_GROWTH_FACTOR,
            shrink_factor: DEFAULT_SHRINK_FACTOR,
            target_latency_us: 0,
            grow_ratio: DEFAULT_GROW_RATIO,
            shrink_ratio: DEFAULT_SHRINK_RATIO,
        }
    }
}

impl AdaptiveChunkConfig {
    /// Validates that all configuration fields are within acceptable ranges.
    ///
    /// Returns `Err` with a descriptive message if:
    /// - `smoothing_factor` is not in the exclusive range (0.0, 1.0)
    /// - `growth_factor` is ≤ 1.0
    /// - `shrink_factor` is not in the exclusive range (0.0, 1.0)
    pub fn validate(&self) -> Result<(), String> {
        if self.smoothing_factor <= 0.0 || self.smoothing_factor >= 1.0 {
            return Err(format!(
                "smoothing_factor must be in (0.0, 1.0), got {}",
                self.smoothing_factor
            ));
        }
        if self.growth_factor <= 1.0 {
            return Err(format!(
                "growth_factor must be > 1.0, got {}",
                self.growth_factor
            ));
        }
        if self.shrink_factor <= 0.0 || self.shrink_factor >= 1.0 {
            return Err(format!(
                "shrink_factor must be in (0.0, 1.0), got {}",
                self.shrink_factor
            ));
        }
        Ok(())
    }
}

/// Lightweight EMA-based throughput measurement for adaptive chunk sizing.
///
/// Tracks producer (`recv`) and consumer (`send`) throughput using exponential
/// moving averages and computes their ratio to inform chunk size decisions.
///
/// Memory budget: 2×f64 (EMA values) + 2×Instant (timestamps) + 1×f64 (alpha) ≤ 64 bytes.
pub(crate) struct ThroughputProbe {
    /// EMA of producer throughput in bytes per second.
    recv_ema_bps: f64,
    /// EMA of consumer throughput in bytes per second.
    send_ema_bps: f64,
    /// Timestamp of the last `on_recv` call.
    last_recv: Instant,
    /// Timestamp of the last `on_send` call.
    last_send: Instant,
    /// EMA smoothing factor (α). Must be in (0.0, 1.0).
    alpha: f64,
}

impl ThroughputProbe {
    /// Creates a new `ThroughputProbe` with the given smoothing factor.
    ///
    /// Both EMA values start at 0.0 (cold start), and timestamps are
    /// initialized to `Instant::now()`.
    pub fn new(alpha: f64) -> Self {
        let now = Instant::now();
        Self {
            recv_ema_bps: 0.0,
            send_ema_bps: 0.0,
            last_recv: now,
            last_send: now,
            alpha,
        }
    }

    /// Records a producer data arrival of `bytes` bytes.
    ///
    /// Computes the instantaneous rate as `bytes / elapsed_seconds` and updates
    /// the receive EMA. If elapsed time since the last call is zero, the update
    /// is skipped to avoid division by zero.
    pub fn on_recv(&mut self, bytes: usize) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_recv);
        let elapsed_secs = elapsed.as_secs_f64();

        if elapsed_secs <= 0.0 {
            return;
        }

        let instantaneous_rate = bytes as f64 / elapsed_secs;
        self.recv_ema_bps = self.alpha * instantaneous_rate + (1.0 - self.alpha) * self.recv_ema_bps;
        self.last_recv = now;
    }

    /// Records a consumer data emission of `bytes` bytes.
    ///
    /// Computes the instantaneous rate as `bytes / elapsed_seconds` and updates
    /// the send EMA. If elapsed time since the last call is zero, the update
    /// is skipped to avoid division by zero.
    pub fn on_send(&mut self, bytes: usize) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_send);
        let elapsed_secs = elapsed.as_secs_f64();

        if elapsed_secs <= 0.0 {
            return;
        }

        let instantaneous_rate = bytes as f64 / elapsed_secs;
        self.send_ema_bps = self.alpha * instantaneous_rate + (1.0 - self.alpha) * self.send_ema_bps;
        self.last_send = now;
    }

    /// Returns the throughput ratio `recv_ema_bps / send_ema_bps`.
    ///
    /// Returns 1.0 (neutral) if `send_ema_bps <= 0.0` or if the computed
    /// ratio is NaN or infinite.
    pub fn ratio(&self) -> f64 {
        if self.send_ema_bps <= 0.0 {
            return 1.0;
        }

        let r = self.recv_ema_bps / self.send_ema_bps;

        if r.is_nan() || r.is_infinite() {
            1.0
        } else {
            r
        }
    }
}

/// Computes the target chunk size based on the throughput ratio and current size.
///
/// This is a pure, deterministic function with no side effects. It implements the
/// resize decision logic for adaptive chunk sizing:
///
/// - When `ratio > grow_ratio`: grows the chunk size by `growth_factor`
/// - When `ratio < shrink_ratio`: shrinks the chunk size by `shrink_factor`
/// - Otherwise (within hysteresis band): keeps the current size
///
/// The result is always clamped to `[min, max]`. Overflow is handled via
/// saturating arithmetic — if the multiplication would exceed `usize::MAX`,
/// the value saturates before clamping to `max`.
///
/// # Parameters
///
/// - `ratio` — throughput ratio (recv_ema / send_ema); NaN/Inf treated as neutral
/// - `current` — current chunk size in bytes
/// - `min` — minimum allowed chunk size (inclusive lower bound)
/// - `max` — maximum allowed chunk size (inclusive upper bound)
/// - `growth_factor` — multiplier when growing (e.g., 2.0)
/// - `shrink_factor` — multiplier when shrinking (e.g., 0.5)
/// - `grow_ratio` — threshold above which to grow (e.g., 1.5)
/// - `shrink_ratio` — threshold below which to shrink (e.g., 0.7)
///
/// # Returns
///
/// The new target chunk size, guaranteed to be in `[min, max]`.
pub(crate) fn compute_target_chunk_size(
    ratio: f64,
    current: usize,
    min: usize,
    max: usize,
    growth_factor: f64,
    shrink_factor: f64,
    grow_ratio: f64,
    shrink_ratio: f64,
) -> usize {
    // Handle NaN or infinite ratio as neutral (no change).
    if ratio.is_nan() || ratio.is_infinite() {
        return current.clamp(min, max);
    }

    let raw = if ratio > grow_ratio {
        // Grow: multiply current by growth_factor with overflow protection.
        let scaled = (current as f64) * growth_factor;
        // If the result exceeds usize::MAX or is NaN/Inf, saturate to usize::MAX.
        if scaled >= usize::MAX as f64 || scaled.is_nan() || scaled.is_infinite() {
            usize::MAX
        } else if scaled < 0.0 {
            0
        } else {
            scaled as usize
        }
    } else if ratio < shrink_ratio {
        // Shrink: multiply current by shrink_factor.
        let scaled = (current as f64) * shrink_factor;
        if scaled >= usize::MAX as f64 || scaled.is_nan() || scaled.is_infinite() {
            usize::MAX
        } else if scaled < 0.0 {
            0
        } else {
            scaled as usize
        }
    } else {
        // Within hysteresis band: no change.
        current
    };

    // Clamp to [min, max] bounds.
    raw.clamp(min, max)
}

/// Spawns an adaptive resizer task that buffers incoming data chunks and
/// re-emits them at a dynamically-adjusted target size based on throughput
/// feedback from a `ThroughputProbe`.
///
/// The function is NOT async — it spawns a Tokio task and returns immediately.
///
/// # Parameters
///
/// - `rx` — input receiver of `StreamChunk` from the upstream stage
/// - `initial_chunk_size` — starting target chunk size before adaptation begins
/// - `min` — minimum allowed chunk size (inclusive lower bound)
/// - `max` — maximum allowed chunk size (inclusive upper bound)
/// - `capacity` — channel capacity for the output receiver
/// - `config` — per-stage adaptive chunking configuration
/// - `stage_name` — label for observability (currently unused, reserved for metrics)
/// - `memory_controller` — optional per-pipeline `MemoryController` for budget-aware
///   resize decisions (§2.12). When present, growth is suppressed under memory pressure
///   and shrink is forced when permits are exhausted.
///
/// # Behavior
///
/// - Buffers incoming `Data` chunks in a `BytesMut` and emits complete chunks
///   at `target_chunk_size`.
/// - On each emission cycle: updates the `ThroughputProbe` and invokes
///   `compute_target_chunk_size` to adjust the target (after at least 3 chunks).
/// - §2.12 budget integration: after computing the new target, if a `MemoryController`
///   is present:
///   - If available permits < 25% of total: suppress growth (keep current size)
///   - If available permits = 0: force shrink (apply shrink_factor)
/// - On `End` or channel closure: flushes remaining buffer as a partial chunk,
///   emits `End`.
/// - On `Error`: forwards the error immediately without flushing.
/// - On downstream drop (send error): terminates the task, releasing the input
///   receiver to propagate cancellation upstream.
pub(crate) fn spawn_adaptive_resizer(
    mut rx: mpsc::Receiver<StreamChunk>,
    initial_chunk_size: usize,
    min: usize,
    max: usize,
    capacity: usize,
    config: AdaptiveChunkConfig,
    stage_name: &str,
    memory_controller: Option<MemoryController>,
) -> mpsc::Receiver<StreamChunk> {
    let (tx, out_rx) = mpsc::channel(capacity);
    let stage_name = stage_name.to_owned();

    tokio::spawn(async move {
        let mut probe = ThroughputProbe::new(config.smoothing_factor);
        let mut target_chunk_size = initial_chunk_size;
        let mut buffer = BytesMut::new();
        let mut chunks_emitted: usize = 0;

        // Record initial chunk size in gauge
        ADAPTIVE_CHUNK_SIZE_BYTES
            .with_label_values(&[&stage_name])
            .set(target_chunk_size as f64);

        loop {
            match rx.recv().await {
                Some(StreamChunk::Data(data)) => {
                    let received_bytes = data.len();
                    probe.on_recv(received_bytes);
                    buffer.extend_from_slice(&data);

                    // Emit complete chunks at target_chunk_size
                    while buffer.len() >= target_chunk_size {
                        let chunk = buffer.split_to(target_chunk_size).freeze();
                        let sent_bytes = chunk.len();

                        if tx.send(StreamChunk::Data(chunk)).await.is_err() {
                            // Downstream dropped — terminate to propagate cancellation
                            // Record final chunk size on exit
                            ADAPTIVE_CHUNK_SIZE_BYTES
                                .with_label_values(&[&stage_name])
                                .set(target_chunk_size as f64);
                            return;
                        }

                        probe.on_send(sent_bytes);
                        chunks_emitted += 1;

                        // Only make resize decisions after at least 3 complete chunks
                        if chunks_emitted >= 3 {
                            let ratio = probe.ratio();
                            let old_target = target_chunk_size;
                            let mut new_target = compute_target_chunk_size(
                                ratio,
                                target_chunk_size,
                                min,
                                max,
                                config.growth_factor,
                                config.shrink_factor,
                                config.grow_ratio,
                                config.shrink_ratio,
                            );

                            // §2.12 Budget-aware override: check memory pressure
                            // BEFORE accepting the computed target.
                            if let Some(ref ctrl) = memory_controller {
                                let available = ctrl.available();
                                let total = ctrl.total_permits();

                                if available == 0 {
                                    // Zero permits available: force shrink regardless
                                    // of what compute_target_chunk_size decided.
                                    let forced_shrink = ((target_chunk_size as f64)
                                        * config.shrink_factor)
                                        as usize;
                                    new_target = forced_shrink.clamp(min, max);
                                    ADAPTIVE_BUDGET_SUPPRESS_TOTAL
                                        .with_label_values(&[&stage_name, "force_shrink"])
                                        .inc();
                                } else if available < total / 4 {
                                    // Available < 25% of total: suppress growth.
                                    // If the computed target is larger, revert to current.
                                    if new_target > target_chunk_size {
                                        new_target = target_chunk_size.clamp(min, max);
                                        ADAPTIVE_BUDGET_SUPPRESS_TOTAL
                                            .with_label_values(&[&stage_name, "suppress_growth"])
                                            .inc();
                                    }
                                }
                            }

                            target_chunk_size = new_target;

                            // Record metrics on resize events
                            if target_chunk_size != old_target {
                                let direction = if target_chunk_size > old_target {
                                    "grow"
                                } else {
                                    "shrink"
                                };
                                ADAPTIVE_RESIZE_EVENTS_TOTAL
                                    .with_label_values(&[&stage_name, direction])
                                    .inc();
                                ADAPTIVE_CHUNK_SIZE_BYTES
                                    .with_label_values(&[&stage_name])
                                    .set(target_chunk_size as f64);
                            }
                        }
                    }
                }

                Some(StreamChunk::Error(e)) => {
                    // Forward error immediately, do NOT flush buffer
                    let _ = tx.send(StreamChunk::Error(e)).await;
                    // Record final chunk size on exit
                    ADAPTIVE_CHUNK_SIZE_BYTES
                        .with_label_values(&[&stage_name])
                        .set(target_chunk_size as f64);
                    return;
                }

                Some(StreamChunk::End) | None => {
                    // End of stream or channel closure: flush remaining buffer
                    if !buffer.is_empty() {
                        let remaining = buffer.freeze();
                        if tx.send(StreamChunk::Data(remaining)).await.is_err() {
                            // Record final chunk size on exit
                            ADAPTIVE_CHUNK_SIZE_BYTES
                                .with_label_values(&[&stage_name])
                                .set(target_chunk_size as f64);
                            return;
                        }
                    }
                    let _ = tx.send(StreamChunk::End).await;
                    // Record final stabilized chunk size on pipeline completion
                    ADAPTIVE_CHUNK_SIZE_BYTES
                        .with_label_values(&[&stage_name])
                        .set(target_chunk_size as f64);
                    return;
                }
            }
        }
    });

    out_rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grow_when_ratio_exceeds_grow_ratio() {
        // ratio 2.0 > grow_ratio 1.5 → current * 2.0 = 128KB
        let result = compute_target_chunk_size(
            2.0,      // ratio
            65_536,   // current (64KB)
            1024,     // min
            1_048_576,// max
            2.0,      // growth_factor
            0.5,      // shrink_factor
            1.5,      // grow_ratio
            0.7,      // shrink_ratio
        );
        assert_eq!(result, 131_072); // 64KB * 2 = 128KB
    }

    #[test]
    fn test_shrink_when_ratio_below_shrink_ratio() {
        // ratio 0.5 < shrink_ratio 0.7 → current * 0.5 = 32KB
        let result = compute_target_chunk_size(
            0.5,      // ratio
            65_536,   // current (64KB)
            1024,     // min
            1_048_576,// max
            2.0,      // growth_factor
            0.5,      // shrink_factor
            1.5,      // grow_ratio
            0.7,      // shrink_ratio
        );
        assert_eq!(result, 32_768); // 64KB * 0.5 = 32KB
    }

    #[test]
    fn test_hold_when_ratio_in_hysteresis_band() {
        // ratio 1.0 is within [0.7, 1.5] → no change
        let result = compute_target_chunk_size(
            1.0,      // ratio
            65_536,   // current (64KB)
            1024,     // min
            1_048_576,// max
            2.0,      // growth_factor
            0.5,      // shrink_factor
            1.5,      // grow_ratio
            0.7,      // shrink_ratio
        );
        assert_eq!(result, 65_536); // unchanged
    }

    #[test]
    fn test_hold_at_exact_grow_ratio_boundary() {
        // ratio == grow_ratio (1.5) → not strictly greater, so hold
        let result = compute_target_chunk_size(
            1.5,      // ratio == grow_ratio
            65_536,   // current
            1024,     // min
            1_048_576,// max
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 65_536);
    }

    #[test]
    fn test_hold_at_exact_shrink_ratio_boundary() {
        // ratio == shrink_ratio (0.7) → not strictly less, so hold
        let result = compute_target_chunk_size(
            0.7,      // ratio == shrink_ratio
            65_536,   // current
            1024,     // min
            1_048_576,// max
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 65_536);
    }

    #[test]
    fn test_clamp_to_max_on_grow() {
        // Growing would exceed max → clamp to max
        let result = compute_target_chunk_size(
            2.0,       // ratio > grow_ratio
            800_000,   // current
            1024,      // min
            1_048_576, // max (1MB)
            2.0,       // growth_factor → 1.6M, exceeds max
            0.5, 1.5, 0.7,
        );
        assert_eq!(result, 1_048_576); // clamped to max
    }

    #[test]
    fn test_clamp_to_min_on_shrink() {
        // Shrinking would go below min → clamp to min
        let result = compute_target_chunk_size(
            0.3,   // ratio < shrink_ratio
            1500,  // current
            1024,  // min
            1_048_576,
            2.0,
            0.5,   // shrink_factor → 750, below min
            1.5, 0.7,
        );
        assert_eq!(result, 1024); // clamped to min
    }

    #[test]
    fn test_overflow_saturates_to_max() {
        // current near usize::MAX → growth would overflow
        let result = compute_target_chunk_size(
            2.0,            // ratio > grow_ratio
            usize::MAX / 2 + 1, // current
            1024,           // min
            1_048_576,      // max
            2.0,            // growth_factor → would overflow
            0.5, 1.5, 0.7,
        );
        assert_eq!(result, 1_048_576); // saturated then clamped to max
    }

    #[test]
    fn test_nan_ratio_returns_current_clamped() {
        let result = compute_target_chunk_size(
            f64::NAN,
            65_536,
            1024,
            1_048_576,
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 65_536); // treated as neutral
    }

    #[test]
    fn test_infinity_ratio_returns_current_clamped() {
        let result = compute_target_chunk_size(
            f64::INFINITY,
            65_536,
            1024,
            1_048_576,
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 65_536); // treated as neutral
    }

    #[test]
    fn test_neg_infinity_ratio_returns_current_clamped() {
        let result = compute_target_chunk_size(
            f64::NEG_INFINITY,
            65_536,
            1024,
            1_048_576,
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 65_536); // treated as neutral
    }

    #[test]
    fn test_current_below_min_gets_clamped_on_hold() {
        // current is below min, ratio in band → clamp to min
        let result = compute_target_chunk_size(
            1.0, 500, 1024, 1_048_576,
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 1024);
    }

    #[test]
    fn test_current_above_max_gets_clamped_on_hold() {
        // current is above max, ratio in band → clamp to max
        let result = compute_target_chunk_size(
            1.0, 2_000_000, 1024, 1_048_576,
            2.0, 0.5, 1.5, 0.7,
        );
        assert_eq!(result, 1_048_576);
    }
}
