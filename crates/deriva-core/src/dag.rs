use crate::address::{CAddr, Recipe};
use crate::error::{DerivaError, Result};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Default)]
pub struct DagStore {
    recipes: HashMap<CAddr, Recipe>,
    forward: HashMap<CAddr, Vec<CAddr>>,
    reverse: HashMap<CAddr, HashSet<CAddr>>,
}

impl DagStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, recipe: Recipe) -> Result<CAddr> {
        let addr = recipe.addr();

        if self.recipes.contains_key(&addr) {
            return Ok(addr);
        }

        if recipe.inputs.contains(&addr) {
            return Err(DerivaError::CycleDetected(format!(
                "recipe {} lists itself as input",
                addr
            )));
        }

        self.forward.insert(addr, recipe.inputs.clone());
        for input in &recipe.inputs {
            self.reverse.entry(*input).or_default().insert(addr);
        }
        self.recipes.insert(addr, recipe);

        Ok(addr)
    }

    pub fn get_recipe(&self, addr: &CAddr) -> Option<&Recipe> {
        self.recipes.get(addr)
    }

    pub fn contains(&self, addr: &CAddr) -> bool {
        self.recipes.contains_key(addr)
    }

    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    pub fn all_addrs(&self) -> Vec<CAddr> {
        self.recipes.keys().copied().collect()
    }

    pub fn remove(&mut self, addr: &CAddr) -> Option<Recipe> {
        let recipe = self.recipes.remove(addr)?;
        self.forward.remove(addr);
        for input in &recipe.inputs {
            if let Some(deps) = self.reverse.get_mut(input) {
                deps.remove(addr);
                if deps.is_empty() {
                    self.reverse.remove(input);
                }
            }
        }
        Some(recipe)
    }

    pub fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        self.reverse
            .get(addr)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
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
        if let Some(inputs) = self.forward.get(addr) {
            for input in inputs {
                self.topo_visit(input, visited, order);
            }
        }
        if self.recipes.contains_key(addr) {
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
        let d = match self.forward.get(addr) {
            None => 0,
            Some(inputs) if inputs.is_empty() => {
                if self.recipes.contains_key(addr) { 1 } else { 0 }
            }
            Some(inputs) => {
                let max_input = inputs.iter().map(|i| self.depth_inner(i, cache)).max().unwrap_or(0);
                max_input + 1
            }
        };
        cache.insert(*addr, d);
        d
    }
}
