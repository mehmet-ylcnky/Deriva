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
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let db = sled::open(root.join("storage.sled"))
            .map_err(|e| deriva_core::error::DerivaError::Storage(e.to_string()))?;
        let recipes = SledRecipeStore::open_with_db(&db)?;
        let blobs = BlobStore::open(root.join("blobs"))?;
        let dag = PersistentDag::open(&db)?;
        Ok(Self { recipes, blobs, dag })
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
}
