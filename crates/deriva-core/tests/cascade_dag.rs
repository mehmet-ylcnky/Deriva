use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::dag::DagStore;
use std::collections::BTreeMap;

fn leaf_addr(name: &str) -> CAddr {
    CAddr::from_bytes(name.as_bytes())
}

fn make_recipe(func: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(FunctionId::new(func, "v1"), inputs, BTreeMap::new())
}

fn insert_recipe(dag: &mut DagStore, func: &str, inputs: Vec<CAddr>) -> CAddr {
    let recipe = make_recipe(func, inputs);
    dag.insert(recipe).unwrap()
}

#[test]
fn depth_tracking_linear_chain() {
    // A → B → C → D
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let b = insert_recipe(&mut dag, "b", vec![a]);
    let c = insert_recipe(&mut dag, "c", vec![b]);
    let d = insert_recipe(&mut dag, "d", vec![c]);

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 3);
    assert_eq!(max_depth, 3);
    assert_eq!(deps[0], (b, 1));
    assert_eq!(deps[1], (c, 2));
    assert_eq!(deps[2], (d, 3));
}

#[test]
fn depth_tracking_diamond() {
    //     A
    //    / \
    //   B   C
    //    \ /
    //     D
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    let b = insert_recipe(&mut dag, "b", vec![a]);
    let c = insert_recipe(&mut dag, "c", vec![a]);
    let d = insert_recipe(&mut dag, "d", vec![b, c]);

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 3);
    assert_eq!(max_depth, 2);
    let depth_1: Vec<_> = deps.iter().filter(|(_, d)| *d == 1).collect();
    let depth_2: Vec<_> = deps.iter().filter(|(_, d)| *d == 2).collect();
    assert_eq!(depth_1.len(), 2);
    assert_eq!(depth_2.len(), 1);
    assert_eq!(depth_2[0].0, d);
}

#[test]
fn depth_tracking_no_dependents() {
    let dag = DagStore::new();
    let a = leaf_addr("a");
    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert!(deps.is_empty());
    assert_eq!(max_depth, 0);
}

#[test]
fn depth_tracking_wide_fanout() {
    let mut dag = DagStore::new();
    let a = leaf_addr("a");
    for i in 0..50 {
        insert_recipe(&mut dag, &format!("r{}", i), vec![a]);
    }

    let (deps, max_depth) = dag.transitive_dependents_with_depth(&a);
    assert_eq!(deps.len(), 50);
    assert_eq!(max_depth, 1);
}
