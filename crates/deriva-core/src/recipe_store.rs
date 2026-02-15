use crate::address::{CAddr, Recipe};
use crate::error::Result;

/// Trait for recipe storage access (used by AsyncExecutor).
pub trait RecipeStore: Send + Sync {
    fn get(&self, addr: &CAddr) -> Result<Option<Recipe>>;
}
