use crate::function::ComputeFunction;
use crate::streaming::StreamingComputeFunction;
use deriva_core::address::FunctionId;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct FunctionRegistry {
    functions: HashMap<FunctionId, Arc<dyn ComputeFunction>>,
    streaming_functions: HashMap<FunctionId, Arc<dyn StreamingComputeFunction>>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, f: Arc<dyn ComputeFunction>) {
        self.functions.insert(f.id(), f);
    }

    pub fn get(&self, id: &FunctionId) -> Option<Arc<dyn ComputeFunction>> {
        self.functions.get(id).cloned()
    }

    pub fn contains(&self, id: &FunctionId) -> bool {
        self.functions.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.functions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    pub fn list(&self) -> Vec<FunctionId> {
        self.functions.keys().cloned().collect()
    }

    pub fn register_streaming(&mut self, f: Arc<dyn StreamingComputeFunction>, id: FunctionId) {
        self.streaming_functions.insert(id, f);
    }

    pub fn get_streaming(&self, id: &FunctionId) -> Option<Arc<dyn StreamingComputeFunction>> {
        self.streaming_functions.get(id).cloned()
    }

    pub fn has_streaming(&self, id: &FunctionId) -> bool {
        self.streaming_functions.contains_key(id)
    }
}
