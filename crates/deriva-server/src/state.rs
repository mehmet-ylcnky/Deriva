use deriva_compute::registry::FunctionRegistry;
use deriva_core::cache::EvictableCache;
use deriva_core::dag::DagStore;
use deriva_storage::StorageBackend;
use std::sync::RwLock;

pub struct ServerState {
    pub dag: RwLock<DagStore>,
    pub cache: RwLock<EvictableCache>,
    pub registry: FunctionRegistry,
    pub storage: StorageBackend,
}

impl ServerState {
    pub fn new(storage: StorageBackend, registry: FunctionRegistry) -> crate::Result<Self> {
        let dag = storage.rebuild_dag()?;
        Ok(Self {
            dag: RwLock::new(dag),
            cache: RwLock::new(EvictableCache::new(Default::default())),
            registry,
            storage,
        })
    }
}
