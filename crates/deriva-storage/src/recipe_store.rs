use deriva_core::address::{CAddr, Recipe};
use deriva_core::dag::DagStore;
use deriva_core::error::{DerivaError, Result};
use deriva_core::recipe_store::RecipeStore;
use sled::Db;

#[derive(Clone)]
pub struct SledRecipeStore {
    db: Db,
}

impl RecipeStore for SledRecipeStore {
    fn get(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        SledRecipeStore::get(self, addr)
    }
}

impl SledRecipeStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let db = sled::open(path).map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(Self { db })
    }

    pub fn open_with_db(db: &sled::Db) -> Result<Self> {
        Ok(Self { db: db.clone() })
    }

    pub fn put(&self, addr: &CAddr, recipe: &Recipe) -> Result<()> {
        let tree = self.db.open_tree("recipes")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        let value = bincode::serialize(recipe)
            .map_err(|e| DerivaError::Serialization(e.to_string()))?;
        tree.insert(addr.as_bytes(), value)
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        let tree = self.db.open_tree("recipes")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        match tree.get(addr.as_bytes()).map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => {
                let recipe: Recipe = bincode::deserialize(&bytes)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                Ok(Some(recipe))
            }
            None => Ok(None),
        }
    }

    pub fn contains(&self, addr: &CAddr) -> Result<bool> {
        let tree = self.db.open_tree("recipes")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        tree.contains_key(addr.as_bytes()).map_err(|e| DerivaError::Storage(e.to_string()))
    }

    pub fn iter_all(&self) -> impl Iterator<Item = Result<(CAddr, Recipe)>> + '_ {
        let tree = match self.db.open_tree("recipes") {
            Ok(t) => t,
            Err(e) => return Box::new(std::iter::once(Err(DerivaError::Storage(e.to_string())))) as Box<dyn Iterator<Item = Result<(CAddr, Recipe)>>>,
        };
        Box::new(tree.iter().map(|result| {
            let (key, value) = result.map_err(|e| DerivaError::Storage(e.to_string()))?;
            let addr = CAddr::from_raw(
                key.as_ref().try_into().map_err(|_| DerivaError::Storage("invalid key length".into()))?
            );
            let recipe: Recipe = bincode::deserialize(&value)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?;
            Ok((addr, recipe))
        }))
    }

    pub fn len(&self) -> usize {
        self.db.open_tree("recipes").map(|t| t.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.db.open_tree("recipes").map(|t| t.is_empty()).unwrap_or(true)
    }

    pub fn remove(&self, addr: &CAddr) -> Result<Option<Recipe>> {
        let tree = self.db.open_tree("recipes")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        match tree.remove(addr.as_bytes()).map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => {
                let recipe: Recipe = bincode::deserialize(&bytes)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                Ok(Some(recipe))
            }
            None => Ok(None),
        }
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn rebuild_dag(&self) -> Result<DagStore> {
        let mut dag = DagStore::new();
        for result in self.iter_all() {
            let (_addr, recipe) = result?;
            dag.insert(recipe)?;
        }
        Ok(dag)
    }
}
