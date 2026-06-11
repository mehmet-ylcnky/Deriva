pub mod blob_store;
pub mod recipe_store;

use deriva_core::address::{CAddr, Recipe};
use deriva_core::error::Result;
use deriva_core::PersistentDag;
use std::path::Path;

pub use blob_store::BlobStore;
pub use recipe_store::SledRecipeStore;

pub struct StorageBackend {
    pub recipes: SledRecipeStore,
    pub blobs: BlobStore,
    pub dag: PersistentDag,
}

impl StorageBackend {
    /// Open the storage backend at the given root path.
    ///
    /// # Startup Behavior
    ///
    /// - **First run (migration)**: If the DAG trees are empty but the recipe store
    ///   contains entries, all recipes are migrated into the PersistentDag. This is
    ///   O(N) but only runs once on upgrade from a pre-DAG version.
    ///
    /// - **Normal startup**: When the DAG is already populated, `open()` completes in
    ///   O(1) — no iteration or rebuild occurs (satisfies Req 2.1–2.3).
    ///
    /// - **Crash recovery**: If a crash occurred between a recipe store write and the
    ///   corresponding DAG insert, call [`repair_consistency()`](Self::repair_consistency)
    ///   after open to detect and fix inconsistencies. This is intentionally NOT called
    ///   automatically to preserve O(1) startup. Operators can invoke it via a startup
    ///   flag, health-check endpoint, or GC RPC.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let db = sled::open(root.join("storage.sled"))
            .map_err(|e| deriva_core::error::DerivaError::Storage(e.to_string()))?;
        let recipes = SledRecipeStore::open_with_db(&db)?;
        let blobs = BlobStore::open(root.join("blobs"))?;
        let dag = PersistentDag::open(&db)?;

        // One-time migration: if DAG trees are empty but recipes exist, populate DAG.
        // After migration the DAG is fully consistent — no repair needed.
        if dag.is_empty() && !recipes.is_empty() {
            tracing::info!("migrating {} recipes to PersistentDag...", recipes.len());
            let mut migrated = 0u64;
            let mut failed = 0u64;
            for result in recipes.iter_all() {
                match result {
                    Ok((_addr, recipe)) => {
                        if let Err(e) = dag.insert(&recipe) {
                            tracing::warn!("migration: skipping recipe {}: {}", recipe.addr(), e);
                            failed += 1;
                        } else {
                            migrated += 1;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("migration: skipping unreadable recipe: {}", e);
                        failed += 1;
                    }
                }
            }
            dag.flush()?;
            tracing::info!("migration complete: {} recipes migrated, {} failed", migrated, failed);
        }
        // When DAG is already populated, startup is O(1): no iteration, no repair.
        // Call `self.repair_consistency()` on-demand for crash recovery scenarios.

        Ok(Self { recipes, blobs, dag })
    }

    /// Open the storage backend and run consistency repair after migration.
    ///
    /// This is a convenience constructor that calls [`repair_consistency()`](Self::repair_consistency)
    /// after the standard open sequence. Use this when crash recovery is a priority
    /// (e.g., after an unclean shutdown) and O(N) startup cost is acceptable.
    ///
    /// For normal production startup where O(1) is required, use [`open()`](Self::open).
    pub fn open_and_repair(root: impl AsRef<Path>) -> Result<Self> {
        let backend = Self::open(root)?;
        let repaired = backend.repair_consistency()?;
        if repaired > 0 {
            tracing::info!("startup repair: {} inconsistent recipes fixed", repaired);
        }
        Ok(backend)
    }

    pub fn put_leaf(&self, data: &[u8]) -> Result<CAddr> {
        let addr = CAddr::from_bytes(data);
        self.blobs.put(&addr, data)?;
        Ok(addr)
    }

    pub fn put_recipe(&self, recipe: &Recipe) -> Result<CAddr> {
        let addr = recipe.addr();
        self.recipes.put(&addr, recipe)?;
        self.dag.insert(recipe)?;
        Ok(addr)
    }

    /// Scan all recipes and re-insert any that are missing from the DAG forward index.
    /// Returns the number of recipes repaired.
    pub fn repair_consistency(&self) -> Result<u64> {
        let mut repaired = 0u64;

        for result in self.recipes.iter_all() {
            let (addr, recipe) = match result {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!("repair_consistency: skipping unreadable recipe: {}", e);
                    continue;
                }
            };

            if self.dag.contains(&addr) {
                continue;
            }

            match self.dag.insert(&recipe) {
                Ok(_) => {
                    repaired += 1;
                }
                Err(e) => {
                    tracing::warn!("repair_consistency: failed to insert recipe {}: {}", addr, e);
                    continue;
                }
            }
        }

        if repaired > 0 {
            self.dag.flush()?;
            tracing::info!("repair_consistency: repaired {} recipes", repaired);
        }

        Ok(repaired)
    }
}
