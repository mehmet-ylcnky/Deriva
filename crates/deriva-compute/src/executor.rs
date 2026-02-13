use crate::cache::MaterializationCache;
use crate::leaf_store::LeafStore;
use crate::registry::FunctionRegistry;
use bytes::Bytes;
use deriva_core::address::CAddr;
use deriva_core::dag::DagStore;
use deriva_core::error::{DerivaError, Result};

pub struct Executor<'a, C: MaterializationCache, L: LeafStore> {
    dag: &'a DagStore,
    registry: &'a FunctionRegistry,
    cache: &'a mut C,
    leaf_store: &'a L,
}

impl<'a, C: MaterializationCache, L: LeafStore> Executor<'a, C, L> {
    pub fn new(
        dag: &'a DagStore,
        registry: &'a FunctionRegistry,
        cache: &'a mut C,
        leaf_store: &'a L,
    ) -> Self {
        Self { dag, registry, cache, leaf_store }
    }

    pub fn materialize(&mut self, addr: &CAddr) -> Result<Bytes> {
        if let Some(bytes) = self.cache.get(addr) {
            return Ok(bytes);
        }

        if let Some(bytes) = self.leaf_store.get_leaf(addr) {
            return Ok(bytes);
        }

        let recipe = self.dag.get_recipe(addr)
            .ok_or_else(|| DerivaError::NotFound(addr.to_string()))?
            .clone();

        let mut input_bytes = Vec::with_capacity(recipe.inputs.len());
        for input_addr in &recipe.inputs {
            input_bytes.push(self.materialize(input_addr)?);
        }

        let func = self.registry.get(&recipe.function_id)
            .ok_or_else(|| DerivaError::FunctionNotFound(recipe.function_id.to_string()))?;

        let output = func.execute(input_bytes, &recipe.params)
            .map_err(|e| DerivaError::ComputeFailed(e.to_string()))?;

        self.cache.put(*addr, output.clone());

        Ok(output)
    }
}
