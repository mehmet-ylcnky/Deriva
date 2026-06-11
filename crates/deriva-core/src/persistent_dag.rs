use crate::address::{CAddr, Recipe};
use crate::dag::DagAccess;
use crate::error::{DerivaError, Result};
use lru::LruCache;
use sled::{Db, Tree, Transactional};
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// Configuration for PersistentDag cache sizes.
pub struct PersistentDagConfig {
    /// Max entries in forward adjacency cache (default: 10,000)
    pub forward_cache_size: NonZeroUsize,
    /// Max entries in reverse adjacency cache (default: 10,000)
    pub reverse_cache_size: NonZeroUsize,
}

impl Default for PersistentDagConfig {
    fn default() -> Self {
        Self {
            forward_cache_size: NonZeroUsize::new(10_000).unwrap(),
            reverse_cache_size: NonZeroUsize::new(10_000).unwrap(),
        }
    }
}

/// Sled-backed DAG with persistent adjacency lists and LRU cache acceleration.
#[derive(Clone)]
pub struct PersistentDag {
    forward: Tree,   // addr → Vec<CAddr> (inputs)
    reverse: Tree,   // addr → Vec<CAddr> (dependents)
    db: Db,
    forward_cache: Arc<Mutex<LruCache<CAddr, Vec<CAddr>>>>,
    reverse_cache: Arc<Mutex<LruCache<CAddr, Vec<CAddr>>>>,
}

impl PersistentDag {
    pub fn open(db: &Db) -> Result<Self> {
        Self::open_with_config(db, PersistentDagConfig::default())
    }

    pub fn open_with_config(db: &Db, config: PersistentDagConfig) -> Result<Self> {
        let forward = db.open_tree("dag_forward")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        let reverse = db.open_tree("dag_reverse")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(Self {
            forward,
            reverse,
            db: db.clone(),
            forward_cache: Arc::new(Mutex::new(LruCache::new(config.forward_cache_size))),
            reverse_cache: Arc::new(Mutex::new(LruCache::new(config.reverse_cache_size))),
        })
    }

    pub fn insert(&self, recipe: &Recipe) -> Result<CAddr> {
        let addr = recipe.addr();
        let addr_bytes = addr.as_bytes();

        // Idempotent: already exists
        if self.forward.contains_key(addr_bytes)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            return Ok(addr);
        }

        // Self-cycle check
        if recipe.inputs.contains(&addr) {
            return Err(DerivaError::CycleDetected(
                format!("recipe {} lists itself as input", addr)
            ));
        }

        // Serialize inputs
        let inputs_bytes = bincode::serialize(&recipe.inputs)
            .map_err(|e| DerivaError::Serialization(e.to_string()))?;

        // Atomic transaction: write forward + update all reverse entries
        let inputs_clone = recipe.inputs.clone();
        let addr_clone = addr;
        
        (&self.forward, &self.reverse).transaction(|(fwd_tx, rev_tx)| {
            // Write forward edge
            fwd_tx.insert(addr_bytes, inputs_bytes.as_slice())?;
            
            // Update reverse edges
            for input in &inputs_clone {
                let key = input.as_bytes();
                let mut deps: Vec<CAddr> = match rev_tx.get(key)? {
                    Some(bytes_ivec) => {
                        let bytes: &[u8] = &bytes_ivec;
                        bincode::deserialize(bytes)
                            .map_err(|_| sled::transaction::ConflictableTransactionError::Abort(()))?
                    }
                    None => Vec::new(),
                };
                if !deps.contains(&addr_clone) {
                    deps.push(addr_clone);
                    let bytes = bincode::serialize(&deps)
                        .map_err(|_| sled::transaction::ConflictableTransactionError::Abort(()))?;
                    rev_tx.insert(key, bytes.as_slice())?;
                }
            }
            
            Ok(())
        }).map_err(|e| DerivaError::Storage(format!("transaction failed: {:?}", e)))?;

        self.db.flush()
            .map_err(|e| DerivaError::Storage(e.to_string()))?;

        // Update caches after successful insert
        {
            let mut cache = self.forward_cache.lock().unwrap();
            cache.put(addr, recipe.inputs.clone());
        }
        // Invalidate reverse cache for each input (their dependents list changed)
        {
            let mut cache = self.reverse_cache.lock().unwrap();
            for input in &recipe.inputs {
                cache.pop(input);
            }
        }

        Ok(addr)
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        self.forward.contains_key(addr.as_bytes()).unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    pub fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        // Check forward_cache first
        {
            let mut cache = self.forward_cache.lock().unwrap();
            if let Some(cached) = cache.get(addr) {
                return Ok(Some(cached.clone()));
            }
        }

        // Cache miss: read from sled
        match self.forward.get(addr.as_bytes())
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => {
                let inputs: Vec<CAddr> = bincode::deserialize(&bytes)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                // Populate cache
                let mut cache = self.forward_cache.lock().unwrap();
                cache.put(*addr, inputs.clone());
                Ok(Some(inputs))
            }
            None => Ok(None),
        }
    }

    pub fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        // Check reverse_cache first
        {
            let mut cache = self.reverse_cache.lock().unwrap();
            if let Some(cached) = cache.get(addr) {
                return cached.clone();
            }
        }

        // Cache miss: read from sled
        match self.reverse.get(addr.as_bytes()) {
            Ok(Some(bytes)) => {
                let deps: Vec<CAddr> = bincode::deserialize(&bytes).unwrap_or_default();
                // Populate cache
                let mut cache = self.reverse_cache.lock().unwrap();
                cache.put(*addr, deps.clone());
                deps
            }
            _ => Vec::new(),
        }
    }

    pub fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        for dep in self.direct_dependents(addr) {
            if visited.insert(dep) {
                queue.push_back(dep);
            }
        }
        while let Some(current) = queue.pop_front() {
            result.push(current);
            for dep in self.direct_dependents(&current) {
                if visited.insert(dep) {
                    queue.push_back(dep);
                }
            }
        }
        result
    }

    pub fn transitive_dependents_with_depth(&self, addr: &CAddr) -> (Vec<(CAddr, u32)>, u32) {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut max_depth: u32 = 0;

        for dep in self.direct_dependents(addr) {
            if visited.insert(dep) {
                queue.push_back((dep, 1u32));
            }
        }
        while let Some((current, depth)) = queue.pop_front() {
            max_depth = max_depth.max(depth);
            result.push((current, depth));
            for dep in self.direct_dependents(&current) {
                if visited.insert(dep) {
                    queue.push_back((dep, depth + 1));
                }
            }
        }
        (result, max_depth)
    }

    pub fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        self.topo_visit(addr, &mut visited, &mut order);
        order
    }

    fn topo_visit(&self, addr: &CAddr, visited: &mut HashSet<CAddr>, order: &mut Vec<CAddr>) {
        if !visited.insert(*addr) {
            return;
        }
        if let Ok(Some(inputs)) = self.inputs(addr) {
            for input in &inputs {
                self.topo_visit(input, visited, order);
            }
            order.push(*addr);
        }
    }

    pub fn depth(&self, addr: &CAddr) -> u32 {
        self.depth_inner(addr, &mut HashMap::new())
    }

    fn depth_inner(&self, addr: &CAddr, cache: &mut HashMap<CAddr, u32>) -> u32 {
        if let Some(&d) = cache.get(addr) {
            return d;
        }
        let d = match self.inputs(addr) {
            Ok(Some(inputs)) if !inputs.is_empty() => {
                let max = inputs.iter()
                    .map(|i| self.depth_inner(i, cache))
                    .max()
                    .unwrap_or(0);
                max + 1
            }
            _ => 0,
        };
        cache.insert(*addr, d);
        d
    }

    pub fn remove(&self, addr: &CAddr) -> Result<bool> {
        let inputs = match self.inputs(addr)? {
            Some(inputs) => inputs,
            None => return Ok(false),
        };

        self.forward.remove(addr.as_bytes())
            .map_err(|e| DerivaError::Storage(e.to_string()))?;

        // Invalidate forward cache for removed address
        {
            let mut cache = self.forward_cache.lock().unwrap();
            cache.pop(addr);
        }

        for input in &inputs {
            self.remove_reverse_edge(input, addr)?;
        }

        Ok(true)
    }

    fn remove_reverse_edge(&self, from: &CAddr, dependent: &CAddr) -> Result<()> {
        let key = from.as_bytes();
        if let Some(bytes) = self.reverse.get(key)
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            let mut deps: Vec<CAddr> = bincode::deserialize(&bytes)
                .map_err(|e| DerivaError::Serialization(e.to_string()))?;
            deps.retain(|d| d != dependent);
            if deps.is_empty() {
                self.reverse.remove(key)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?;
            } else {
                let bytes = bincode::serialize(&deps)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                self.reverse.insert(key, bytes)
                    .map_err(|e| DerivaError::Storage(e.to_string()))?;
            }
            // Invalidate reverse cache for this entry
            let mut cache = self.reverse_cache.lock().unwrap();
            cache.pop(from);
        }
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(())
    }

    /// List all recipe output addrs in the DAG.
    pub fn all_addrs(&self) -> Vec<CAddr> {
        self.forward.iter()
            .filter_map(|r| r.ok())
            .filter_map(|(k, _)| <[u8; 32]>::try_from(k.as_ref()).ok().map(CAddr::from_raw))
            .collect()
    }

    /// Compute the set of all live addrs: recipe outputs ∪ their inputs.
    pub fn live_addr_set(&self) -> HashSet<CAddr> {
        let mut live = HashSet::new();
        for (key, val) in self.forward.iter().flatten() {
            if let Ok(arr) = <[u8; 32]>::try_from(key.as_ref()) {
                live.insert(CAddr::from_raw(arr));
            }
            if let Ok(inputs) = bincode::deserialize::<Vec<CAddr>>(&val) {
                for input in inputs {
                    live.insert(input);
                }
            }
        }
        live
    }
}

impl DagAccess for PersistentDag {
    fn insert(&self, recipe: &Recipe) -> Result<CAddr> {
        PersistentDag::insert(self, recipe)
    }

    fn contains(&self, addr: &CAddr) -> bool {
        PersistentDag::contains(self, addr)
    }

    fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        PersistentDag::inputs(self, addr)
    }

    fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        PersistentDag::direct_dependents(self, addr)
    }

    fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        PersistentDag::transitive_dependents(self, addr)
    }

    fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr> {
        PersistentDag::resolve_order(self, addr)
    }

    fn depth(&self, addr: &CAddr) -> u32 {
        PersistentDag::depth(self, addr)
    }

    fn len(&self) -> usize {
        PersistentDag::len(self)
    }

    fn is_empty(&self) -> bool {
        PersistentDag::is_empty(self)
    }

    fn remove(&self, addr: &CAddr) -> Result<bool> {
        PersistentDag::remove(self, addr)
    }
}
