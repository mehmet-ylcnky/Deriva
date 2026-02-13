use deriva_core::address::*;
use deriva_core::dag::DagStore;
use std::collections::BTreeMap;

fn leaf(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

fn recipe(name: &str, ver: &str, inputs: Vec<CAddr>, params: BTreeMap<String, Value>) -> Recipe {
    Recipe::new(FunctionId::new(name, ver), inputs, params)
}

// --- Test Group 1: Basic insert and lookup ---

#[test]
fn insert_and_retrieve() {
    let mut dag = DagStore::new();
    let r = recipe("compress", "1.0.0", vec![leaf("data")], BTreeMap::new());
    let addr = dag.insert(r.clone()).unwrap();
    assert_eq!(dag.get_recipe(&addr), Some(&r));
}

#[test]
fn insert_idempotent() {
    let mut dag = DagStore::new();
    let r = recipe("compress", "1.0.0", vec![leaf("data")], BTreeMap::new());
    let a1 = dag.insert(r.clone()).unwrap();
    let a2 = dag.insert(r).unwrap();
    assert_eq!(a1, a2);
    assert_eq!(dag.len(), 1);
}

#[test]
fn contains_and_len() {
    let mut dag = DagStore::new();
    let r = recipe("fn", "1.0.0", vec![leaf("x")], BTreeMap::new());
    let addr = dag.insert(r).unwrap();
    assert!(dag.contains(&addr));
    assert!(!dag.contains(&leaf("nonexistent")));
    assert_eq!(dag.len(), 1);
    assert!(!dag.is_empty());
}

#[test]
fn empty_dag() {
    let dag = DagStore::new();
    assert!(dag.is_empty());
    assert_eq!(dag.len(), 0);
    assert!(dag.all_addrs().is_empty());
}

#[test]
fn get_recipe_missing() {
    let dag = DagStore::new();
    assert_eq!(dag.get_recipe(&leaf("nope")), None);
}

// --- Test Group 2: Cycle detection ---

#[test]
fn reject_direct_self_reference() {
    let mut dag = DagStore::new();
    let r = recipe("fn", "1.0.0", vec![leaf("a")], BTreeMap::new());
    let addr = r.addr();
    // Insert a recipe whose input is addr of another recipe — should succeed
    let evil = Recipe::new(FunctionId::new("fn", "1.0.0"), vec![addr], BTreeMap::new());
    assert!(dag.insert(evil).is_ok());
}

// --- Test Group 3: Remove ---

#[test]
fn remove_recipe() {
    let mut dag = DagStore::new();
    let r = recipe("fn", "1.0.0", vec![leaf("x")], BTreeMap::new());
    let addr = dag.insert(r.clone()).unwrap();
    assert_eq!(dag.remove(&addr), Some(r));
    assert!(!dag.contains(&addr));
    assert_eq!(dag.len(), 0);
}

#[test]
fn remove_nonexistent() {
    let mut dag = DagStore::new();
    assert_eq!(dag.remove(&leaf("nope")), None);
}

#[test]
fn remove_cleans_reverse_index() {
    let mut dag = DagStore::new();
    let input = leaf("data");
    let r = recipe("fn", "1.0.0", vec![input], BTreeMap::new());
    let addr = dag.insert(r).unwrap();
    assert!(dag.direct_dependents(&input).contains(&addr));
    dag.remove(&addr);
    assert!(dag.direct_dependents(&input).is_empty());
}

// --- Test Group 4: Dependents ---

#[test]
fn direct_dependents_single() {
    let mut dag = DagStore::new();
    let input = leaf("raw");
    let r = recipe("compress", "1.0.0", vec![input], BTreeMap::new());
    let addr = dag.insert(r).unwrap();
    assert_eq!(dag.direct_dependents(&input), vec![addr]);
}

#[test]
fn direct_dependents_multiple() {
    let mut dag = DagStore::new();
    let input = leaf("shared");
    let a1 = dag.insert(recipe("fn_a", "1.0.0", vec![input], BTreeMap::new())).unwrap();
    let a2 = dag.insert(recipe("fn_b", "1.0.0", vec![input], BTreeMap::new())).unwrap();
    let mut deps = dag.direct_dependents(&input);
    deps.sort();
    let mut expected = vec![a1, a2];
    expected.sort();
    assert_eq!(deps, expected);
}

#[test]
fn direct_dependents_none() {
    let dag = DagStore::new();
    assert!(dag.direct_dependents(&leaf("orphan")).is_empty());
}

#[test]
fn transitive_dependents_chain() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![c], BTreeMap::new())).unwrap();
    assert_eq!(dag.transitive_dependents(&a), vec![b, c, d]);
}

#[test]
fn transitive_dependents_diamond() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![b, c], BTreeMap::new())).unwrap();
    let deps = dag.transitive_dependents(&a);
    assert_eq!(deps.len(), 3);
    assert!(deps.contains(&b));
    assert!(deps.contains(&c));
    assert_eq!(*deps.last().unwrap(), d);
}

#[test]
fn transitive_dependents_no_duplicates() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    let e = dag.insert(recipe("e", "1.0.0", vec![c, d], BTreeMap::new())).unwrap();
    let deps = dag.transitive_dependents(&a);
    assert_eq!(deps.len(), 4);
    let unique: std::collections::HashSet<_> = deps.iter().collect();
    assert_eq!(unique.len(), 4);
}

// --- Test Group 5: Topological ordering ---

#[test]
fn resolve_order_single_recipe() {
    let mut dag = DagStore::new();
    let r = recipe("fn", "1.0.0", vec![leaf("x")], BTreeMap::new());
    let addr = dag.insert(r).unwrap();
    assert_eq!(dag.resolve_order(&addr), vec![addr]);
}

#[test]
fn resolve_order_chain() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    assert_eq!(dag.resolve_order(&c), vec![b, c]);
}

#[test]
fn resolve_order_diamond() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![b, c], BTreeMap::new())).unwrap();
    let order = dag.resolve_order(&d);
    assert_eq!(order.len(), 3);
    let d_pos = order.iter().position(|x| *x == d).unwrap();
    let b_pos = order.iter().position(|x| *x == b).unwrap();
    let c_pos = order.iter().position(|x| *x == c).unwrap();
    assert!(b_pos < d_pos);
    assert!(c_pos < d_pos);
}

#[test]
fn resolve_order_excludes_leaves() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let order = dag.resolve_order(&b);
    assert!(!order.contains(&a));
    assert_eq!(order, vec![b]);
}

#[test]
fn resolve_order_leaf_returns_empty() {
    let dag = DagStore::new();
    assert!(dag.resolve_order(&leaf("not_in_dag")).is_empty());
}

#[test]
fn resolve_order_deep_chain() {
    let mut dag = DagStore::new();
    let mut prev = leaf("start");
    let mut addrs = Vec::new();
    for i in 0..5 {
        let r = recipe(&format!("step{i}"), "1.0.0", vec![prev], BTreeMap::new());
        prev = dag.insert(r).unwrap();
        addrs.push(prev);
    }
    assert_eq!(dag.resolve_order(&addrs[4]), addrs);
}

// --- Test Group 6: Depth ---

#[test]
fn depth_leaf() {
    let dag = DagStore::new();
    assert_eq!(dag.depth(&leaf("x")), 0);
}

#[test]
fn depth_single_recipe() {
    let mut dag = DagStore::new();
    let addr = dag.insert(recipe("fn", "1.0.0", vec![leaf("x")], BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&addr), 1);
}

#[test]
fn depth_chain() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![c], BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&a), 0);
    assert_eq!(dag.depth(&b), 1);
    assert_eq!(dag.depth(&c), 2);
    assert_eq!(dag.depth(&d), 3);
}

#[test]
fn depth_diamond() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![b, c], BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&d), 2);
}

#[test]
fn depth_asymmetric() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = leaf("B");
    let c = dag.insert(recipe("c", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let d = dag.insert(recipe("d", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    let e = dag.insert(recipe("e", "1.0.0", vec![c], BTreeMap::new())).unwrap();
    let f = dag.insert(recipe("f", "1.0.0", vec![e, d], BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&c), 1);
    assert_eq!(dag.depth(&d), 1);
    assert_eq!(dag.depth(&e), 2);
    assert_eq!(dag.depth(&f), 3);
}

#[test]
fn depth_recipe_no_inputs() {
    let mut dag = DagStore::new();
    let addr = dag.insert(recipe("generate", "1.0.0", vec![], BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&addr), 1);
}

// --- Test Group 7: all_addrs ---

#[test]
fn all_addrs_returns_all_recipes() {
    let mut dag = DagStore::new();
    let a1 = dag.insert(recipe("a", "1.0.0", vec![leaf("x")], BTreeMap::new())).unwrap();
    let a2 = dag.insert(recipe("b", "1.0.0", vec![leaf("y")], BTreeMap::new())).unwrap();
    let mut addrs = dag.all_addrs();
    addrs.sort();
    let mut expected = vec![a1, a2];
    expected.sort();
    assert_eq!(addrs, expected);
}

// --- Test Group 8: Edge cases ---

#[test]
fn many_dependents_on_single_input() {
    let mut dag = DagStore::new();
    let shared = leaf("shared");
    let mut addrs = Vec::new();
    for i in 0..100 {
        addrs.push(dag.insert(recipe(&format!("fn_{i}"), "1.0.0", vec![shared], BTreeMap::new())).unwrap());
    }
    let deps = dag.direct_dependents(&shared);
    assert_eq!(deps.len(), 100);
    for a in &addrs {
        assert!(deps.contains(a));
    }
}

#[test]
fn wide_fan_in() {
    let mut dag = DagStore::new();
    let inputs: Vec<CAddr> = (0..50).map(|i| leaf(&format!("input_{i}"))).collect();
    let addr = dag.insert(recipe("merge", "1.0.0", inputs.clone(), BTreeMap::new())).unwrap();
    assert_eq!(dag.depth(&addr), 1);
    assert_eq!(dag.resolve_order(&addr), vec![addr]);
    for input in &inputs {
        assert!(dag.direct_dependents(input).contains(&addr));
    }
}

#[test]
fn remove_middle_of_chain_breaks_traversal() {
    let mut dag = DagStore::new();
    let a = leaf("A");
    let b = dag.insert(recipe("b", "1.0.0", vec![a], BTreeMap::new())).unwrap();
    let c = dag.insert(recipe("c", "1.0.0", vec![b], BTreeMap::new())).unwrap();
    dag.remove(&b);
    assert!(dag.contains(&c));
    assert!(!dag.contains(&b));
    assert_eq!(dag.resolve_order(&c), vec![c]);
}
