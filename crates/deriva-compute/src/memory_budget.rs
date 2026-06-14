//! Memory budget enforcement for streaming pipelines (§2.12).
//!
//! Provides [`MemoryController`] which wraps a `tokio::sync::Semaphore` to limit
//! total in-flight data chunks across all channels in a pipeline. Each permit
//! represents one chunk (`chunk_size` bytes) of in-flight data.

use std::sync::Arc;
use deriva_core::StreamChunk;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics::{MEMORY_BUDGET_BYTES, MEMORY_BUDGET_UTILIZATION, MEMORY_BUDGET_WAIT_TOTAL};

/// Controls memory usage for a single streaming pipeline by limiting
/// the number of in-flight chunks via a semaphore.
///
/// Each permit represents one chunk-sized unit of in-flight data.
/// Producers acquire a permit before sending; consumers release it
/// when they finish processing (via `ChunkGuard` drop).
///
/// `MemoryController` is cheap to clone (wraps an `Arc<Semaphore>`).
#[derive(Clone, Debug)]
pub struct MemoryController {
    semaphore: Arc<Semaphore>,
    total_permits: usize,
}

impl MemoryController {
    /// Create a new `MemoryController` with permits computed as `budget / chunk_size`.
    ///
    /// If `budget < chunk_size`, permits are clamped to 1 and a warning is logged
    /// to ensure the pipeline can always make forward progress.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    pub fn new(budget: usize, chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk_size must be > 0");

        let permits = if budget < chunk_size {
            tracing::warn!(
                budget,
                chunk_size,
                "memory budget ({budget}) is less than chunk_size ({chunk_size}); clamping to 1 permit"
            );
            1
        } else {
            let p = budget / chunk_size;
            // Safety: budget >= chunk_size and chunk_size > 0, so p >= 1.
            p.max(1)
        };

        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            total_permits: permits,
        }
    }

    /// Acquire a permit, waiting asynchronously if all permits are currently held.
    ///
    /// This implements backpressure: when the budget is exhausted, the producer
    /// awaits here until a consumer releases a permit by dropping its `ChunkGuard`.
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("MemoryController semaphore closed unexpectedly")
    }

    /// Try to acquire a permit without waiting.
    ///
    /// Returns `Some(permit)` if a permit is available, `None` otherwise.
    pub fn try_acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().ok()
    }

    /// Returns the number of permits currently available (not held by in-flight chunks).
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Returns the total number of permits configured for this controller.
    pub fn total_permits(&self) -> usize {
        self.total_permits
    }
}

/// Controls memory usage across ALL concurrent streaming pipelines by limiting
/// the total number of in-flight chunks system-wide via a shared semaphore.
///
/// Each permit represents one chunk-sized unit of in-flight data. When the
/// global budget is exhausted, producers across all pipelines experience
/// backpressure until consumers release permits.
///
/// Stored in `ServerState` as `Arc<Option<GlobalMemoryController>>`. When
/// `global_memory_budget = 0`, the option is `None` (unlimited mode).
///
/// `GlobalMemoryController` is cheap to clone (wraps an `Arc<Semaphore>`).
#[derive(Clone, Debug)]
pub struct GlobalMemoryController {
    semaphore: Arc<Semaphore>,
    total_permits: usize,
}

impl GlobalMemoryController {
    /// Create a new `GlobalMemoryController` with permits computed as
    /// `global_budget / chunk_size`.
    ///
    /// The "None when budget=0" logic is handled at the call site (e.g.,
    /// `ServerState`), not here. This constructor expects `global_budget > 0`.
    ///
    /// If `global_budget < chunk_size`, permits are clamped to 1 and a warning
    /// is logged to ensure at least one pipeline can make forward progress.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is 0.
    pub fn new(global_budget: usize, chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk_size must be > 0");

        let permits = if global_budget < chunk_size {
            tracing::warn!(
                global_budget,
                chunk_size,
                "global_memory_budget ({global_budget}) is less than chunk_size ({chunk_size}); clamping to 1 permit"
            );
            1
        } else {
            let p = global_budget / chunk_size;
            // Safety: global_budget >= chunk_size and chunk_size > 0, so p >= 1.
            p.max(1)
        };

        MEMORY_BUDGET_BYTES.set(global_budget as f64);

        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            total_permits: permits,
        }
    }

    /// Acquire a permit from the global budget, waiting asynchronously if all
    /// permits are currently held across all pipelines.
    ///
    /// This implements cross-pipeline backpressure: when the global budget is
    /// exhausted, any producer in any pipeline awaits here until a consumer
    /// somewhere releases a permit by dropping its `ChunkGuard`.
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("GlobalMemoryController semaphore closed unexpectedly")
    }

    /// Try to acquire a permit from the global budget without waiting.
    ///
    /// Returns `Some(permit)` if a permit is available, `None` otherwise.
    /// Useful for metrics/pressure checking.
    pub fn try_acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().ok()
    }

    /// Returns the number of permits currently available (not held by in-flight
    /// chunks across all pipelines).
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Returns the total number of permits configured for the global budget.
    pub fn total_permits(&self) -> usize {
        self.total_permits
    }
}

/// A guard that pairs a [`StreamChunk`] with an optional semaphore permit.
///
/// When a `ChunkGuard` is dropped, the held `OwnedSemaphorePermit` (if any)
/// is released back to the semaphore, and the utilization metric is updated.
///
/// The `permit` field is `None` for sentinel chunks (`End` / `Error`) that
/// are sent without acquiring a permit.
#[derive(Debug)]
pub struct ChunkGuard {
    /// The stream chunk (Data, End, or Error).
    pub chunk: StreamChunk,
    /// Held permit — released on drop. `None` for sentinel chunks.
    permit: Option<OwnedSemaphorePermit>,
    /// Optional controller reference for updating utilization metrics on drop.
    controller: Option<MemoryController>,
}

impl ChunkGuard {
    /// Create a new `ChunkGuard` wrapping a chunk and an optional permit.
    ///
    /// - For `Data` chunks: pass `Some(permit)` acquired from a `MemoryController`.
    /// - For `End` / `Error` chunks: pass `None` (sentinel bypass).
    /// - `controller`: pass `Some(controller)` to enable utilization metric updates on drop.
    pub fn new(
        chunk: StreamChunk,
        permit: Option<OwnedSemaphorePermit>,
        controller: Option<MemoryController>,
    ) -> Self {
        Self {
            chunk,
            permit,
            controller,
        }
    }
}

impl Drop for ChunkGuard {
    fn drop(&mut self) {
        // Drop the permit first to release it back to the semaphore.
        self.permit.take();
        // Now update the utilization metric if we have a controller reference.
        if let Some(ref controller) = self.controller {
            let utilization =
                1.0 - (controller.available() as f64 / controller.total_permits() as f64);
            MEMORY_BUDGET_UTILIZATION.set(utilization);
        }
    }
}

// ---------------------------------------------------------------------------
// BudgetedSender / BudgetedReceiver
// ---------------------------------------------------------------------------

/// A channel sender that enforces memory budget accounting.
///
/// Before sending a `Data` chunk, [`BudgetedSender`] acquires a permit from the
/// pipeline's [`MemoryController`]. Sentinel chunks (`End` / `Error`) bypass
/// permit acquisition — they carry no data payload and must never be blocked.
///
/// If the receiver is dropped, `send()` returns an error and the permit (if
/// acquired) is automatically dropped (released back to the semaphore).
#[derive(Clone, Debug)]
pub struct BudgetedSender {
    tx: mpsc::Sender<(StreamChunk, Option<OwnedSemaphorePermit>)>,
    controller: MemoryController,
}

impl BudgetedSender {
    /// Send a [`StreamChunk`] through the budgeted channel.
    ///
    /// - `Data` chunks: acquires a permit (blocks if budget exhausted), then sends.
    /// - `End` / `Error` chunks: sent immediately without a permit (sentinel bypass).
    ///
    /// Returns `Err(SendError<StreamChunk>)` if the receiver has been dropped.
    pub async fn send(&self, chunk: StreamChunk) -> Result<(), SendError<StreamChunk>> {
        match &chunk {
            StreamChunk::Data(_) => {
                // Try to acquire without blocking to detect backpressure.
                let permit = match self.controller.try_acquire() {
                    Some(permit) => permit,
                    None => {
                        // Would block — record backpressure event.
                        MEMORY_BUDGET_WAIT_TOTAL.inc();
                        self.controller.acquire().await
                    }
                };
                // Update utilization metric after acquiring.
                MEMORY_BUDGET_UTILIZATION.set(
                    1.0 - (self.controller.available() as f64
                        / self.controller.total_permits() as f64),
                );
                // Send the chunk with its permit through the channel.
                self.tx
                    .send((chunk, Some(permit)))
                    .await
                    .map_err(|e| SendError(e.0 .0))
            }
            StreamChunk::End | StreamChunk::Error(_) => {
                // Sentinel bypass: no permit needed.
                self.tx
                    .send((chunk, None))
                    .await
                    .map_err(|e| SendError(e.0 .0))
            }
        }
    }

    /// Returns a reference to the underlying [`MemoryController`].
    pub fn controller(&self) -> &MemoryController {
        &self.controller
    }
}

/// A channel receiver that returns [`ChunkGuard`] values.
///
/// Each received chunk is wrapped in a `ChunkGuard` that holds the associated
/// semaphore permit (if any). The permit is released when the guard is dropped
/// after the consumer finishes processing.
#[derive(Debug)]
pub struct BudgetedReceiver {
    rx: mpsc::Receiver<(StreamChunk, Option<OwnedSemaphorePermit>)>,
    controller: MemoryController,
}

impl BudgetedReceiver {
    /// Receive the next chunk from the channel, wrapped in a [`ChunkGuard`].
    ///
    /// Returns `None` when the sender has been dropped and all buffered messages
    /// have been consumed.
    pub async fn recv(&mut self) -> Option<ChunkGuard> {
        self.rx
            .recv()
            .await
            .map(|(chunk, permit)| ChunkGuard::new(chunk, permit, Some(self.controller.clone())))
    }
}

/// Create a budgeted channel pair with the given capacity and controller.
///
/// The returned [`BudgetedSender`] enforces permit acquisition for `Data` chunks.
/// The returned [`BudgetedReceiver`] yields [`ChunkGuard`] values that release
/// permits on drop.
///
/// `capacity` governs the underlying `mpsc` channel buffer size (per-channel
/// backpressure). The `controller` governs cross-channel budget backpressure.
pub fn budgeted_channel(
    capacity: usize,
    controller: MemoryController,
) -> (BudgetedSender, BudgetedReceiver) {
    let (tx, rx) = mpsc::channel(capacity);
    (
        BudgetedSender {
            tx,
            controller: controller.clone(),
        },
        BudgetedReceiver { rx, controller },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_computed_from_budget_and_chunk_size() {
        let mc = MemoryController::new(10 * 1024 * 1024, 64 * 1024); // 10MB / 64KB = 160
        assert_eq!(mc.total_permits(), 160);
        assert_eq!(mc.available(), 160);
    }

    #[test]
    fn budget_less_than_chunk_size_clamps_to_one() {
        let mc = MemoryController::new(100, 64 * 1024); // 100 < 64KB
        assert_eq!(mc.total_permits(), 1);
        assert_eq!(mc.available(), 1);
    }

    #[test]
    fn budget_equal_to_chunk_size_gives_one_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024);
        assert_eq!(mc.total_permits(), 1);
    }

    #[test]
    fn try_acquire_succeeds_when_available() {
        let mc = MemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let p1 = mc.try_acquire();
        assert!(p1.is_some());
        assert_eq!(mc.available(), 1);
    }

    #[test]
    fn try_acquire_returns_none_when_exhausted() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let _p1 = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);
        assert!(mc.try_acquire().is_none());
    }

    #[test]
    fn drop_permit_releases_back_to_semaphore() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let p = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);
        drop(p);
        assert_eq!(mc.available(), 1);
    }

    #[tokio::test]
    async fn acquire_blocks_and_resumes_on_release() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let p1 = mc.acquire().await;
        assert_eq!(mc.available(), 0);

        let mc_clone = mc.clone();
        let handle = tokio::spawn(async move {
            // This will block until p1 is dropped
            let _p2 = mc_clone.acquire().await;
            // permit is released when _p2 drops at end of block
        });

        // Give the spawned task a chance to reach the await point
        tokio::task::yield_now().await;

        // Release the permit — the spawned task should now be able to acquire
        drop(p1);

        // The spawned task should now complete (it acquires and then drops the permit)
        handle.await.unwrap();
        // After the spawned task completes, its permit is dropped, so available = 1
        assert_eq!(mc.available(), 1);
    }

    #[test]
    fn clone_shares_same_semaphore() {
        let mc1 = MemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let mc2 = mc1.clone();

        let _p = mc1.try_acquire().unwrap();
        assert_eq!(mc2.available(), 1); // shared semaphore
    }

    #[test]
    #[should_panic(expected = "chunk_size must be > 0")]
    fn zero_chunk_size_panics() {
        MemoryController::new(1024, 0);
    }

    // --- ChunkGuard tests ---

    #[test]
    fn chunk_guard_new_with_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let permit = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);

        let guard = ChunkGuard::new(
            StreamChunk::Data(bytes::Bytes::from_static(b"hello")),
            Some(permit),
            Some(mc.clone()),
        );
        assert!(guard.chunk.is_data());
        assert_eq!(mc.available(), 0);
    }

    #[test]
    fn chunk_guard_drop_releases_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let permit = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);

        let guard = ChunkGuard::new(
            StreamChunk::Data(bytes::Bytes::from_static(b"data")),
            Some(permit),
            Some(mc.clone()),
        );
        drop(guard);
        assert_eq!(mc.available(), 1); // permit released
    }

    #[test]
    fn chunk_guard_sentinel_no_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        // End chunk with no permit
        let guard = ChunkGuard::new(StreamChunk::End, None, None);
        assert!(guard.chunk.is_end());
        // No permit held — available unchanged
        assert_eq!(mc.available(), 1);
        drop(guard);
        assert_eq!(mc.available(), 1);
    }

    // --- BudgetedSender / BudgetedReceiver tests ---

    #[tokio::test]
    async fn budgeted_send_data_acquires_permit() {
        let mc = MemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let (sender, mut receiver) = budgeted_channel(8, mc.clone());

        assert_eq!(mc.available(), 2);
        sender
            .send(StreamChunk::Data(bytes::Bytes::from_static(b"hello")))
            .await
            .unwrap();
        // Permit is held in-channel (not yet received)
        assert_eq!(mc.available(), 1);

        let guard = receiver.recv().await.unwrap();
        assert!(guard.chunk.is_data());
        // Still held by the guard
        assert_eq!(mc.available(), 1);

        drop(guard);
        // Released
        assert_eq!(mc.available(), 2);
    }

    #[tokio::test]
    async fn budgeted_send_end_does_not_acquire_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        // Exhaust all permits
        let _held = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);

        let (sender, mut receiver) = budgeted_channel(8, mc.clone());

        // End should succeed even with 0 available permits
        sender.send(StreamChunk::End).await.unwrap();

        let guard = receiver.recv().await.unwrap();
        assert!(guard.chunk.is_end());
        // Still 0 — no permit was used
        assert_eq!(mc.available(), 0);
    }

    #[tokio::test]
    async fn budgeted_send_error_does_not_acquire_permit() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let _held = mc.try_acquire().unwrap();
        assert_eq!(mc.available(), 0);

        let (sender, mut receiver) = budgeted_channel(8, mc.clone());

        let err = deriva_core::DerivaError::ComputeFailed("test".into());
        sender.send(StreamChunk::Error(err)).await.unwrap();

        let guard = receiver.recv().await.unwrap();
        assert!(guard.chunk.is_error());
        assert_eq!(mc.available(), 0);
    }

    #[tokio::test]
    async fn budgeted_send_returns_error_when_receiver_dropped() {
        let mc = MemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let (sender, receiver) = budgeted_channel(8, mc.clone());

        drop(receiver);

        let result = sender
            .send(StreamChunk::Data(bytes::Bytes::from_static(b"data")))
            .await;
        assert!(result.is_err());
        // Permit was acquired then dropped because send failed — available back to 2
        assert_eq!(mc.available(), 2);
    }

    #[tokio::test]
    async fn budgeted_receiver_returns_none_when_sender_dropped() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024);
        let (sender, mut receiver) = budgeted_channel(8, mc);

        drop(sender);
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn budgeted_channel_backpressure() {
        let mc = MemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let (sender, mut receiver) = budgeted_channel(8, mc.clone());

        // First send acquires the only permit
        sender
            .send(StreamChunk::Data(bytes::Bytes::from_static(b"first")))
            .await
            .unwrap();
        assert_eq!(mc.available(), 0);

        // Second send should block because 0 permits available.
        // Use a timeout to verify it blocks.
        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            sender_clone
                .send(StreamChunk::Data(bytes::Bytes::from_static(b"second")))
                .await
                .unwrap();
        });

        // Give the spawned task a moment to reach the acquire await
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        // Still blocked
        assert!(!handle.is_finished());

        // Consume first chunk — releases its permit
        let guard = receiver.recv().await.unwrap();
        drop(guard);

        // Now the spawned send should complete
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn budgeted_channel_factory_creates_working_pair() {
        let mc = MemoryController::new(256 * 1024, 64 * 1024); // 4 permits
        let (sender, mut receiver) = budgeted_channel(4, mc.clone());

        for i in 0..4u8 {
            sender
                .send(StreamChunk::Data(bytes::Bytes::from(vec![i; 100])))
                .await
                .unwrap();
        }
        assert_eq!(mc.available(), 0);

        // Consume all
        for _ in 0..4 {
            let guard = receiver.recv().await.unwrap();
            assert!(guard.chunk.is_data());
            drop(guard);
        }
        assert_eq!(mc.available(), 4);
    }

    // --- GlobalMemoryController tests ---

    #[test]
    fn global_permits_computed_from_budget_and_chunk_size() {
        // 256MB / 64KB = 4096 permits
        let gmc = GlobalMemoryController::new(256 * 1024 * 1024, 64 * 1024);
        assert_eq!(gmc.total_permits(), 4096);
        assert_eq!(gmc.available(), 4096);
    }

    #[test]
    fn global_budget_less_than_chunk_size_clamps_to_one() {
        let gmc = GlobalMemoryController::new(100, 64 * 1024); // 100 < 64KB
        assert_eq!(gmc.total_permits(), 1);
        assert_eq!(gmc.available(), 1);
    }

    #[test]
    fn global_budget_equal_to_chunk_size_gives_one_permit() {
        let gmc = GlobalMemoryController::new(64 * 1024, 64 * 1024);
        assert_eq!(gmc.total_permits(), 1);
    }

    #[test]
    fn global_try_acquire_succeeds_when_available() {
        let gmc = GlobalMemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let p1 = gmc.try_acquire();
        assert!(p1.is_some());
        assert_eq!(gmc.available(), 1);
    }

    #[test]
    fn global_try_acquire_returns_none_when_exhausted() {
        let gmc = GlobalMemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let _p1 = gmc.try_acquire().unwrap();
        assert_eq!(gmc.available(), 0);
        assert!(gmc.try_acquire().is_none());
    }

    #[test]
    fn global_drop_permit_releases_back_to_semaphore() {
        let gmc = GlobalMemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let p = gmc.try_acquire().unwrap();
        assert_eq!(gmc.available(), 0);
        drop(p);
        assert_eq!(gmc.available(), 1);
    }

    #[tokio::test]
    async fn global_acquire_blocks_and_resumes_on_release() {
        let gmc = GlobalMemoryController::new(64 * 1024, 64 * 1024); // 1 permit
        let p1 = gmc.acquire().await;
        assert_eq!(gmc.available(), 0);

        let gmc_clone = gmc.clone();
        let handle = tokio::spawn(async move {
            // This will block until p1 is dropped
            let _p2 = gmc_clone.acquire().await;
        });

        // Give the spawned task a chance to reach the await point
        tokio::task::yield_now().await;

        // Release the permit — the spawned task should now be able to acquire
        drop(p1);

        handle.await.unwrap();
        // After the spawned task completes, its permit is dropped, so available = 1
        assert_eq!(gmc.available(), 1);
    }

    #[test]
    fn global_clone_shares_same_semaphore() {
        let gmc1 = GlobalMemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let gmc2 = gmc1.clone();

        let _p = gmc1.try_acquire().unwrap();
        assert_eq!(gmc2.available(), 1); // shared semaphore
    }

    #[test]
    #[should_panic(expected = "chunk_size must be > 0")]
    fn global_zero_chunk_size_panics() {
        GlobalMemoryController::new(256 * 1024 * 1024, 0);
    }

    #[test]
    fn global_controller_shared_across_pipelines_via_arc() {
        // Simulates how ServerState holds Arc<Option<GlobalMemoryController>>
        let gmc = GlobalMemoryController::new(128 * 1024, 64 * 1024); // 2 permits
        let shared: Arc<Option<GlobalMemoryController>> = Arc::new(Some(gmc));

        // Pipeline A acquires a permit
        let permit_a = shared.as_ref().as_ref().unwrap().try_acquire().unwrap();
        assert_eq!(shared.as_ref().as_ref().unwrap().available(), 1);

        // Pipeline B acquires a permit
        let permit_b = shared.as_ref().as_ref().unwrap().try_acquire().unwrap();
        assert_eq!(shared.as_ref().as_ref().unwrap().available(), 0);

        // No more permits — budget exhausted across both pipelines
        assert!(shared.as_ref().as_ref().unwrap().try_acquire().is_none());

        // Pipeline A completes — releases its permit
        drop(permit_a);
        assert_eq!(shared.as_ref().as_ref().unwrap().available(), 1);

        // Pipeline B completes — releases its permit
        drop(permit_b);
        assert_eq!(shared.as_ref().as_ref().unwrap().available(), 2);
    }

    #[test]
    fn global_budget_zero_means_none_at_call_site() {
        // This test demonstrates the "None when budget=0" pattern used in ServerState
        let global_memory_budget: usize = 0;
        let chunk_size: usize = 64 * 1024;

        let controller: Option<GlobalMemoryController> = if global_memory_budget == 0 {
            None
        } else {
            Some(GlobalMemoryController::new(global_memory_budget, chunk_size))
        };

        assert!(controller.is_none());
    }

    #[test]
    fn global_budget_nonzero_creates_controller() {
        let global_memory_budget: usize = 256 * 1024 * 1024; // 256MB
        let chunk_size: usize = 64 * 1024;

        let controller: Option<GlobalMemoryController> = if global_memory_budget == 0 {
            None
        } else {
            Some(GlobalMemoryController::new(global_memory_budget, chunk_size))
        };

        assert!(controller.is_some());
        let gmc = controller.unwrap();
        assert_eq!(gmc.total_permits(), 4096);
    }
}
