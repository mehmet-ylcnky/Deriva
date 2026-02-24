use bytes::Bytes;
use deriva_compute::builtins::*;
use deriva_compute::cache::MaterializationCache;
use deriva_compute::function::ComputeFunction;
use deriva_compute::leaf_store::LeafStore;
use deriva_compute::registry::FunctionRegistry;
use deriva_compute::Executor;
use deriva_core::address::*;
use deriva_core::dag::DagStore;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// --- Test helpers ---

#[derive(Default)]
struct MemCache {
    store: HashMap<CAddr, Bytes>,
}

impl MaterializationCache for MemCache {
    fn get(&mut self, addr: &CAddr) -> Option<Bytes> {
        self.store.get(addr).cloned()
    }
    fn put(&mut self, addr: CAddr, data: Bytes) -> u64 {
        let size = data.len() as u64;
        self.store.insert(addr, data);
        size
    }
    fn contains(&self, addr: &CAddr) -> bool {
        self.store.contains_key(addr)
    }
}

#[derive(Default)]
struct MemLeafStore {
    store: HashMap<CAddr, Bytes>,
}

impl MemLeafStore {
    fn insert(&mut self, data: &[u8]) -> CAddr {
        let addr = CAddr::from_bytes(data);
        self.store.insert(addr, Bytes::from(data.to_vec()));
        addr
    }
}

impl LeafStore for MemLeafStore {
    fn get_leaf(&self, addr: &CAddr) -> Option<Bytes> {
        self.store.get(addr).cloned()
    }
}

// --- Test Group 1: FunctionRegistry ---

#[test]
fn register_and_lookup() {
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(IdentityFn));
    assert!(reg.get(&FunctionId::new("identity", "1.0.0")).is_some());
}

#[test]
fn lookup_missing() {
    let reg = FunctionRegistry::new();
    assert!(reg.get(&FunctionId::new("nope", "1.0.0")).is_none());
}

#[test]
fn registry_contains() {
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(ConcatFn));
    assert!(reg.contains(&FunctionId::new("concat", "1.0.0")));
    assert!(!reg.contains(&FunctionId::new("concat", "2.0.0")));
}

#[test]
fn registry_len_and_list() {
    let mut reg = FunctionRegistry::new();
    assert!(reg.is_empty());
    reg.register(Arc::new(IdentityFn));
    reg.register(Arc::new(ConcatFn));
    assert_eq!(reg.len(), 2);
    let ids = reg.list();
    assert!(ids.contains(&FunctionId::new("identity", "1.0.0")));
    assert!(ids.contains(&FunctionId::new("concat", "1.0.0")));
}

#[test]
fn register_overwrites_same_id() {
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(IdentityFn));
    reg.register(Arc::new(IdentityFn));
    assert_eq!(reg.len(), 1);
}

// --- Test Group 2: Built-in functions ---

#[test]
fn identity_returns_input() {
    let r = IdentityFn.execute(vec![Bytes::from("hello")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("hello"));
}

#[test]
fn identity_rejects_zero_inputs() {
    assert!(IdentityFn.execute(vec![], &BTreeMap::new()).is_err());
}

#[test]
fn identity_rejects_multiple_inputs() {
    assert!(IdentityFn.execute(vec![Bytes::from("a"), Bytes::from("b")], &BTreeMap::new()).is_err());
}

#[test]
fn concat_joins_inputs() {
    let r = ConcatFn.execute(vec![Bytes::from("hello "), Bytes::from("world")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("hello world"));
}

#[test]
fn concat_empty_inputs() {
    let r = ConcatFn.execute(vec![], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::new());
}

#[test]
fn concat_single_input() {
    let r = ConcatFn.execute(vec![Bytes::from("only")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("only"));
}

#[test]
fn concat_many_inputs() {
    let inputs: Vec<Bytes> = (0..10).map(|i| Bytes::from(format!("{i}"))).collect();
    let r = ConcatFn.execute(inputs, &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("0123456789"));
}

#[test]
fn uppercase_works() {
    let r = UppercaseFn.execute(vec![Bytes::from("hello world")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("HELLO WORLD"));
}

#[test]
fn uppercase_already_upper() {
    let r = UppercaseFn.execute(vec![Bytes::from("ABC")], &BTreeMap::new()).unwrap();
    assert_eq!(r, Bytes::from("ABC"));
}

#[test]
fn uppercase_rejects_invalid_utf8() {
    assert!(UppercaseFn.execute(vec![Bytes::from(vec![0xFF, 0xFE])], &BTreeMap::new()).is_err());
}

#[test]
fn repeat_works() {
    let params = BTreeMap::from([("count".into(), Value::Int(3))]);
    let r = RepeatFn.execute(vec![Bytes::from("ab")], &params).unwrap();
    assert_eq!(r, Bytes::from("ababab"));
}

#[test]
fn repeat_once() {
    let params = BTreeMap::from([("count".into(), Value::Int(1))]);
    let r = RepeatFn.execute(vec![Bytes::from("x")], &params).unwrap();
    assert_eq!(r, Bytes::from("x"));
}

#[test]
fn repeat_missing_param() {
    assert!(RepeatFn.execute(vec![Bytes::from("x")], &BTreeMap::new()).is_err());
}

#[test]
fn repeat_zero_count() {
    let params = BTreeMap::from([("count".into(), Value::Int(0))]);
    assert!(RepeatFn.execute(vec![Bytes::from("x")], &params).is_err());
}

#[test]
fn repeat_negative_count() {
    let params = BTreeMap::from([("count".into(), Value::Int(-5))]);
    assert!(RepeatFn.execute(vec![Bytes::from("x")], &params).is_err());
}

#[test]
fn repeat_wrong_param_type() {
    let params = BTreeMap::from([("count".into(), Value::String("three".into()))]);
    assert!(RepeatFn.execute(vec![Bytes::from("x")], &params).is_err());
}

// --- Test Group 3: Determinism ---

#[test]
fn identity_is_deterministic() {
    let input = vec![Bytes::from("determinism test")];
    let r1 = IdentityFn.execute(input.clone(), &BTreeMap::new()).unwrap();
    let r2 = IdentityFn.execute(input, &BTreeMap::new()).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn concat_is_deterministic() {
    let inputs = vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")];
    let r1 = ConcatFn.execute(inputs.clone(), &BTreeMap::new()).unwrap();
    let r2 = ConcatFn.execute(inputs, &BTreeMap::new()).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn uppercase_is_deterministic() {
    let input = vec![Bytes::from("Hello World 123!")];
    let r1 = UppercaseFn.execute(input.clone(), &BTreeMap::new()).unwrap();
    let r2 = UppercaseFn.execute(input, &BTreeMap::new()).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn repeat_is_deterministic() {
    let input = vec![Bytes::from("abc")];
    let params = BTreeMap::from([("count".into(), Value::Int(5))]);
    let r1 = RepeatFn.execute(input.clone(), &params).unwrap();
    let r2 = RepeatFn.execute(input, &params).unwrap();
    assert_eq!(r1, r2);
}

// --- Test Group 4: Executor — leaf resolution ---

#[test]
fn materialize_leaf() {
    let dag = DagStore::new();
    let reg = FunctionRegistry::new();
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let addr = leaves.insert(b"raw data");

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&addr).unwrap(), Bytes::from("raw data"));
}

#[test]
fn materialize_missing_addr() {
    let dag = DagStore::new();
    let reg = FunctionRegistry::new();
    let mut cache = MemCache::default();
    let leaves = MemLeafStore::default();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert!(exec.materialize(&CAddr::from_bytes(b"nonexistent")).is_err());
}

// --- Test Group 5: Executor — single-level derived ---

#[test]
fn materialize_identity_of_leaf() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(IdentityFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"hello");

    let addr = dag.insert(Recipe::new(FunctionId::new("identity", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&addr).unwrap(), Bytes::from("hello"));
}

#[test]
fn materialize_uppercase_of_leaf() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"hello");

    let addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&addr).unwrap(), Bytes::from("HELLO"));
}

#[test]
fn materialize_concat_two_leaves() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(ConcatFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let a = leaves.insert(b"hello ");
    let b = leaves.insert(b"world");

    let addr = dag.insert(Recipe::new(FunctionId::new("concat", "1.0.0"), vec![a, b], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&addr).unwrap(), Bytes::from("hello world"));
}

#[test]
fn materialize_repeat_with_params() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(RepeatFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"ab");

    let addr = dag.insert(Recipe::new(
        FunctionId::new("repeat", "1.0.0"),
        vec![leaf_addr],
        BTreeMap::from([("count".into(), Value::Int(3))]),
    )).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&addr).unwrap(), Bytes::from("ababab"));
}

// --- Test Group 6: Executor — multi-level DAG ---

#[test]
fn materialize_chain_uppercase_then_repeat() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    reg.register(Arc::new(RepeatFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"hello");

    let upper_addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();
    let repeat_addr = dag.insert(Recipe::new(
        FunctionId::new("repeat", "1.0.0"),
        vec![upper_addr],
        BTreeMap::from([("count".into(), Value::Int(3))]),
    )).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&repeat_addr).unwrap(), Bytes::from("HELLOHELLOHELLO"));
}

#[test]
fn materialize_diamond_dag() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(ConcatFn));
    reg.register(Arc::new(UppercaseFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let a = leaves.insert(b"hello ");
    let b = leaves.insert(b"world");

    let concat_addr = dag.insert(Recipe::new(FunctionId::new("concat", "1.0.0"), vec![a, b], BTreeMap::new())).unwrap();
    let upper_addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![concat_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&upper_addr).unwrap(), Bytes::from("HELLO WORLD"));
}

#[test]
fn materialize_three_levels_deep() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(IdentityFn));
    reg.register(Arc::new(UppercaseFn));
    reg.register(Arc::new(RepeatFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"hi");

    let id_addr = dag.insert(Recipe::new(FunctionId::new("identity", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();
    let upper_addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![id_addr], BTreeMap::new())).unwrap();
    let repeat_addr = dag.insert(Recipe::new(
        FunctionId::new("repeat", "1.0.0"),
        vec![upper_addr],
        BTreeMap::from([("count".into(), Value::Int(2))]),
    )).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert_eq!(exec.materialize(&repeat_addr).unwrap(), Bytes::from("HIHI"));
}

// --- Test Group 7: Executor — caching behavior ---

#[test]
fn materialize_caches_result() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"test");

    let addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    exec.materialize(&addr).unwrap();

    assert!(cache.contains(&addr));
    assert_eq!(cache.get(&addr).unwrap(), Bytes::from("TEST"));
}

#[test]
fn materialize_uses_cache_on_second_call() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"test");

    let addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    let r1 = exec.materialize(&addr).unwrap();
    let r2 = exec.materialize(&addr).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn materialize_intermediate_results_cached() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    reg.register(Arc::new(RepeatFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"hi");

    let upper_addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();
    let repeat_addr = dag.insert(Recipe::new(
        FunctionId::new("repeat", "1.0.0"),
        vec![upper_addr],
        BTreeMap::from([("count".into(), Value::Int(2))]),
    )).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    exec.materialize(&repeat_addr).unwrap();

    assert!(cache.contains(&upper_addr), "intermediate result should be cached");
    assert!(cache.contains(&repeat_addr), "final result should be cached");
}

// --- Test Group 8: Executor — error cases ---

#[test]
fn materialize_missing_function() {
    let mut dag = DagStore::new();
    let reg = FunctionRegistry::new();
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(b"data");

    let addr = dag.insert(Recipe::new(FunctionId::new("nonexistent", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    let err = exec.materialize(&addr).unwrap_err();
    assert!(err.to_string().contains("not registered") || err.to_string().contains("nonexistent"));
}

#[test]
fn materialize_function_execution_error() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(UppercaseFn));
    let mut cache = MemCache::default();
    let mut leaves = MemLeafStore::default();
    let leaf_addr = leaves.insert(&[0xFF, 0xFE]);

    let addr = dag.insert(Recipe::new(FunctionId::new("uppercase", "1.0.0"), vec![leaf_addr], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert!(exec.materialize(&addr).is_err());
}

#[test]
fn materialize_missing_input_leaf() {
    let mut dag = DagStore::new();
    let mut reg = FunctionRegistry::new();
    reg.register(Arc::new(IdentityFn));
    let mut cache = MemCache::default();
    let leaves = MemLeafStore::default();

    let phantom = CAddr::from_bytes(b"not stored");
    let addr = dag.insert(Recipe::new(FunctionId::new("identity", "1.0.0"), vec![phantom], BTreeMap::new())).unwrap();

    let mut exec = Executor::new(&dag, &reg, &mut cache, &leaves);
    assert!(exec.materialize(&addr).is_err());
}

// --- Test Group 9: ComputeCost ---

#[test]
fn identity_cost_is_zero() {
    let cost = IdentityFn.estimated_cost(&[1024]);
    assert_eq!(cost.cpu_ms, 0);
    assert_eq!(cost.memory_bytes, 0);
}

#[test]
fn concat_cost_scales_with_input() {
    let cost = ConcatFn.estimated_cost(&[100, 200, 300]);
    assert_eq!(cost.memory_bytes, 600);
}

#[test]
fn uppercase_cost_scales_with_input() {
    let cost = UppercaseFn.estimated_cost(&[1000]);
    assert_eq!(cost.memory_bytes, 1000);
}
