pub mod blob_store;
pub mod recipe_store;

use deriva_core::address::{CAddr, Recipe};
use deriva_core::dag::DagStore;
use deriva_core::error::Result;
use std::path::Path;

pub use blob_store::BlobStore;
pub use recipe_store::SledRecipeStore;

pub struct StorageBackend {
    pub recipes: SledRecipeStore,
    pub blobs: BlobStore,
}

impl StorageBackend {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let recipes = SledRecipeStore::open(root.join("recipes.sled"))?;
        let blobs = BlobStore::open(root.join("blobs"))?;
        Ok(Self { recipes, blobs })
    }

    pub fn put_leaf(&self, data: &[u8]) -> Result<CAddr> {
        let addr = CAddr::from_bytes(data);
        self.blobs.put(&addr, data)?;
        Ok(addr)
    }

    pub fn put_recipe(&self, recipe: &Recipe) -> Result<CAddr> {
        let addr = recipe.addr();
        self.recipes.put(&addr, recipe)?;
        Ok(addr)
    }

    pub fn rebuild_dag(&self) -> Result<DagStore> {
        self.recipes.rebuild_dag()
    }
}
