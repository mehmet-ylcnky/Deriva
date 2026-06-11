use crate::address::{CAddr, Recipe};
use crate::error::{DerivaError, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;

/// Trait abstracting DAG operations for testability.
/// Both PersistentDag and DagStore implement this trait.
pub trait DagAccess: Send + Sync {
    fn insert(&self, recipe: &Recipe) -> Result<CAddr>;
    fn contains(&self, addr: &CAddr) -> bool;
    fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>>;
    fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr>;
    fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr>;
    fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr>;
    fn depth(&self, addr: &CAddr) -> u32;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn remove(&self, addr: &CAddr) -> Result<bool>;
}

#[derive(Debug, Default)]
struct DagStoreInner {
    recipes: HashMap<CAddr, Recipe>,
    forward: HashMap<CAddr, Vec<CAddr>>,
    reverse: HashMap<CAddr, HashSet<CAddr>>,
}

#[derive(Debug, Default)]
pub struct DagStore {
    inner: RwLock<DagStoreInner>,
}

impl DagStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, recipe: impl std::borrow::Borrow<Recipe>) -> Result<CAddr> {
        let recipe = recipe.borrow();
        let addr = recipe.addr();

        // Check with read lock first for the fast idempotent path
        {
            let inner = self.inner.read().unwrap();
            if inner.recipes.contains_key(&addr) {
                return Ok(addr);
            }
        }

        if recipe.inputs.contains(&addr) {
            return Err(DerivaError::CycleDetected(format!(
                "recipe {} lists itself as input",
                addr
            )));
        }

        let mut inner = self.inner.write().unwrap();

        // Double-check after acquiring write lock (another thread may have inserted)
        if inner.recipes.contains_key(&addr) {
            return Ok(addr);
        }

        inner.forward.insert(addr, recipe.inputs.clone());
        for input in &recipe.inputs {
            inner.reverse.entry(*input).or_default().insert(addr);
        }
        inner.recipes.insert(addr, recipe.clone());

        Ok(addr)
    }

    pub fn get_recipe(&self, addr: &CAddr) -> Option<Recipe> {
        let inner = self.inner.read().unwrap();
        inner.recipes.get(addr).cloned()
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        let inner = self.inner.read().unwrap();
        inner.recipes.contains_key(addr)
    }

    pub fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        let inner = self.inner.read().unwrap();
        Ok(inner.forward.get(addr).cloned())
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.recipes.len()
    }

    pub fn is_empty(&self) -> bool {
        let inner = self.inner.read().unwrap();
        inner.recipes.is_empty()
    }

    pub fn all_addrs(&self) -> Vec<CAddr> {
        let inner = self.inner.read().unwrap();
        inner.recipes.keys().copied().collect()
    }

    pub fn remove(&self, addr: &CAddr) -> Result<bool> {
        let mut inner = self.inner.write().unwrap();
        let recipe = match inner.recipes.remove(addr) {
            Some(r) => r,
            None => return Ok(false),
        };
        inner.forward.remove(addr);
        for input in &recipe.inputs {
            if let Some(deps) = inner.reverse.get_mut(input) {
                deps.remove(addr);
                if deps.is_empty() {
                    inner.reverse.remove(input);
                }
            }
        }
        Ok(true)
    }

    pub fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        let inner = self.inner.read().unwrap();
        inner
            .reverse
            .get(addr)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        let inner = self.inner.read().unwrap();
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(deps) = inner.reverse.get(addr) {
            for &dep in deps {
                if visited.insert(dep) {
                    queue.push_back(dep);
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            result.push(current);
            if let Some(deps) = inner.reverse.get(&current) {
                for &dep in deps {
                    if visited.insert(dep) {
                        queue.push_back(dep);
                    }
                }
            }
        }

        result
    }

    pub fn transitive_dependents_with_depth(&self, addr: &CAddr) -> (Vec<(CAddr, u32)>, u32) {
        let inner = self.inner.read().unwrap();
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut max_depth: u32 = 0;

        if let Some(deps) = inner.reverse.get(addr) {
            for &dep in deps {
                if visited.insert(dep) {
                    queue.push_back((dep, 1u32));
                }
            }
        }

        while let Some((current, depth)) = queue.pop_front() {
            max_depth = max_depth.max(depth);
            result.push((current, depth));
            if let Some(deps) = inner.reverse.get(&current) {
                for &dep in deps {
                    if visited.insert(dep) {
                        queue.push_back((dep, depth + 1));
                    }
                }
            }
        }

        (result, max_depth)
    }

    pub fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr> {
        let inner = self.inner.read().unwrap();
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        Self::topo_visit_inner(&inner, addr, &mut visited, &mut order);
        order
    }

    fn topo_visit_inner(
        inner: &DagStoreInner,
        addr: &CAddr,
        visited: &mut HashSet<CAddr>,
        order: &mut Vec<CAddr>,
    ) {
        if !visited.insert(*addr) {
            return;
        }
        if let Some(inputs) = inner.forward.get(addr) {
            for input in inputs {
                Self::topo_visit_inner(inner, input, visited, order);
            }
        }
        if inner.recipes.contains_key(addr) {
            order.push(*addr);
        }
    }

    pub fn depth(&self, addr: &CAddr) -> u32 {
        let inner = self.inner.read().unwrap();
        Self::depth_inner_recursive(&inner, addr, &mut HashMap::new())
    }

    fn depth_inner_recursive(
        inner: &DagStoreInner,
        addr: &CAddr,
        cache: &mut HashMap<CAddr, u32>,
    ) -> u32 {
        if let Some(&d) = cache.get(addr) {
            return d;
        }
        let d = match inner.forward.get(addr) {
            None => 0,
            Some(inputs) if inputs.is_empty() => {
                if inner.recipes.contains_key(addr) {
                    1
                } else {
                    0
                }
            }
            Some(inputs) => {
                let max_input = inputs
                    .iter()
                    .map(|i| Self::depth_inner_recursive(inner, i, cache))
                    .max()
                    .unwrap_or(0);
                max_input + 1
            }
        };
        cache.insert(*addr, d);
        d
    }
}

impl DagAccess for DagStore {
    fn insert(&self, recipe: &Recipe) -> Result<CAddr> {
        DagStore::insert(self, recipe)
    }

    fn contains(&self, addr: &CAddr) -> bool {
        DagStore::contains(self, addr)
    }

    fn inputs(&self, addr: &CAddr) -> Result<Option<Vec<CAddr>>> {
        DagStore::inputs(self, addr)
    }

    fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        DagStore::direct_dependents(self, addr)
    }

    fn transitive_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        DagStore::transitive_dependents(self, addr)
    }

    fn resolve_order(&self, addr: &CAddr) -> Vec<CAddr> {
        DagStore::resolve_order(self, addr)
    }

    fn depth(&self, addr: &CAddr) -> u32 {
        DagStore::depth(self, addr)
    }

    fn len(&self) -> usize {
        DagStore::len(self)
    }

    fn is_empty(&self) -> bool {
        DagStore::is_empty(self)
    }

    fn remove(&self, addr: &CAddr) -> Result<bool> {
        DagStore::remove(self, addr)
    }
}
