use crate::address::{CAddr, Recipe};
use crate::error::{DerivaError, Result};
use sled::{Db, Tree, Transactional};
use std::collections::{HashMap, HashSet, VecDeque};

/// Sled-backed DAG with persistent adjacency lists.
pub struct PersistentDag {
    forward: Tree,   // addr → Vec<CAddr> (inputs)
    reverse: Tree,   // addr → Vec<CAddr> (dependents)
    db: Db,
}

impl PersistentDag {
    pub fn open(db: &Db) -> Result<Self> {
        let forward = db.open_tree("dag_forward")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        let reverse = db.open_tree("dag_reverse")
            .map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(Self {
            forward,
            reverse,
            db: db.clone(),
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
        match self.forward.get(addr.as_bytes())
            .map_err(|e| DerivaError::Storage(e.to_string()))? {
            Some(bytes) => {
                let inputs: Vec<CAddr> = bincode::deserialize(&bytes)
                    .map_err(|e| DerivaError::Serialization(e.to_string()))?;
                Ok(Some(inputs))
            }
            None => Ok(None),
        }
    }

    pub fn direct_dependents(&self, addr: &CAddr) -> Vec<CAddr> {
        match self.reverse.get(addr.as_bytes()) {
            Ok(Some(bytes)) => {
                bincode::deserialize(&bytes).unwrap_or_default()
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
        }
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.db.flush().map_err(|e| DerivaError::Storage(e.to_string()))?;
        Ok(())
    }
}
