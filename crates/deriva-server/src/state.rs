use deriva_compute::async_executor::{AsyncExecutor, CombinedDagReader, ExecutorConfig, VerificationMode};
use deriva_compute::cache::SharedCache;
use deriva_compute::memory_budget::GlobalMemoryController;
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::streaming::DEFAULT_CHUNK_SIZE;
use deriva_core::cache::EvictableCache;
use deriva_core::gc::PinSet;
use deriva_core::PersistentDag;
use deriva_storage::{StorageBackend, SledRecipeStore};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Default global memory budget: 256MB (268,435,456 bytes).
pub const DEFAULT_GLOBAL_MEMORY_BUDGET: usize = 268_435_456;

pub struct ServerState {
    pub executor: AsyncExecutor<SharedCache, deriva_storage::BlobStore, CombinedDagReader<SledRecipeStore>>,
    pub cache: Arc<SharedCache>,
    pub dag: Arc<PersistentDag>,
    pub recipes: Arc<SledRecipeStore>,
    pub blobs: Arc<deriva_storage::BlobStore>,
    pub registry: Arc<FunctionRegistry>,
    pub storage: StorageBackend,
    pub pins: Arc<RwLock<PinSet>>,
    pub start_time: Instant,
    pub global_memory_controller: Arc<Option<GlobalMemoryController>>,
}

impl ServerState {
    pub fn new(storage: StorageBackend, registry: FunctionRegistry) -> crate::Result<Self> {
        Self::with_verification(storage, registry, VerificationMode::Off)
    }

    pub fn with_verification(
        storage: StorageBackend,
        registry: FunctionRegistry,
        verification: VerificationMode,
    ) -> crate::Result<Self> {
        Self::with_global_budget(storage, registry, verification, DEFAULT_GLOBAL_MEMORY_BUDGET)
    }

    pub fn with_global_budget(
        storage: StorageBackend,
        registry: FunctionRegistry,
        verification: VerificationMode,
        global_memory_budget: usize,
    ) -> crate::Result<Self> {
        let cache = Arc::new(SharedCache::new(EvictableCache::new(Default::default())));
        let dag = Arc::new(storage.dag.clone());
        let recipes = Arc::new(storage.recipes.clone());
        let blobs = Arc::new(storage.blobs.clone());
        let registry = Arc::new(registry);

        let dag_reader = Arc::new(CombinedDagReader {
            dag: Arc::clone(&dag),
            recipes: Arc::clone(&recipes),
        });

        let config = ExecutorConfig {
            verification,
            ..Default::default()
        };

        let executor = AsyncExecutor::with_config(
            dag_reader,
            Arc::clone(&registry),
            Arc::clone(&cache),
            Arc::clone(&blobs),
            config,
        );

        let global_memory_controller = Arc::new(if global_memory_budget == 0 {
            None
        } else {
            Some(GlobalMemoryController::new(global_memory_budget, DEFAULT_CHUNK_SIZE))
        });

        Ok(Self {
            executor,
            cache,
            dag,
            recipes,
            blobs,
            registry,
            storage,
            pins: Arc::new(RwLock::new(PinSet::new())),
            start_time: Instant::now(),
            global_memory_controller,
        })
    }
}
