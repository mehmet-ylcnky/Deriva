use deriva_compute::async_executor::{AsyncExecutor, CombinedDagReader, ExecutorConfig, VerificationMode};
use deriva_compute::cache::SharedCache;
use deriva_compute::registry::FunctionRegistry;
use deriva_core::cache::EvictableCache;
use deriva_core::PersistentDag;
use deriva_storage::{StorageBackend, SledRecipeStore};
use std::sync::Arc;

pub struct ServerState {
    pub executor: AsyncExecutor<SharedCache, deriva_storage::BlobStore, CombinedDagReader<SledRecipeStore>>,
    pub cache: Arc<SharedCache>,
    pub dag: Arc<PersistentDag>,
    pub recipes: Arc<SledRecipeStore>,
    pub registry: Arc<FunctionRegistry>,
    pub storage: StorageBackend,
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
            blobs,
            config,
        );

        Ok(Self {
            executor,
            cache,
            dag,
            recipes,
            registry,
            storage,
        })
    }
}
