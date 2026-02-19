use std::collections::BTreeMap;
use deriva_core::address::{CAddr, FunctionId, Recipe};
use deriva_core::gc::PinSet;
use deriva_core::persistent_dag::PersistentDag;
use deriva_compute::gc::compute_live_set;

fn temp_dag() -> PersistentDag {
    let db = sled::Config::new().temporary(true).open().unwrap();
    PersistentDag::open(&db).unwrap()
}

fn make_recipe(name: &str, inputs: Vec<CAddr>) -> Recipe {
    Recipe::new(FunctionId::new(name, "1"), inputs, BTreeMap::new())
}

fn leaf(seed: u8) -> CAddr {
    CAddr::from_raw([seed; 32])
}

#[test]
fn live_set_from_single_recipe() {
    let dag = temp_dag();
    let a = leaf(1);
    let r = make_recipe("f", vec![a]);
    let out = dag.insert(&r).unwrap();
    let live = compute_live_set(&dag, &PinSet::new());
    assert!(live.contains(&a));
    assert!(live.contains(&out));
    assert_eq!(live.len(), 2);
}

#[test]
fn live_set_from_chain() {
    let dag = temp_dag();
    let a = leaf(1);
    let r1 = make_recipe("f1", vec![a]);
    let x = dag.insert(&r1).unwrap();
    let r2 = make_recipe("f2", vec![x]);
    let y = dag.insert(&r2).unwrap();
    let live = compute_live_set(&dag, &PinSet::new());
    assert!(live.contains(&a));
    assert!(live.contains(&x));
    assert!(live.contains(&y));
    assert_eq!(live.len(), 3);
}

#[test]
fn live_set_with_shared_input() {
    let dag = temp_dag();
    let a = leaf(1);
    let r1 = make_recipe("f1", vec![a]);
    let x = dag.insert(&r1).unwrap();
    let r2 = make_recipe("f2", vec![a]);
    let y = dag.insert(&r2).unwrap();
    let live = compute_live_set(&dag, &PinSet::new());
    assert!(live.contains(&a));
    assert!(live.contains(&x));
    assert!(live.contains(&y));
    assert_eq!(live.len(), 3);
}

#[test]
fn live_set_pins_only() {
    let dag = temp_dag();
    let mut pins = PinSet::new();
    let p = leaf(99);
    pins.pin(p);
    let live = compute_live_set(&dag, &pins);
    assert!(live.contains(&p));
    assert_eq!(live.len(), 1);
}

#[test]
fn live_set_empty_dag_empty_pins() {
    let dag = temp_dag();
    let live = compute_live_set(&dag, &PinSet::new());
    assert!(live.is_empty());
}

#[test]
fn live_set_pins_merge_with_dag() {
    let dag = temp_dag();
    let a = leaf(1);
    let r = make_recipe("f", vec![a]);
    let out = dag.insert(&r).unwrap();
    let mut pins = PinSet::new();
    let p = leaf(50);
    pins.pin(p);
    let live = compute_live_set(&dag, &pins);
    assert!(live.contains(&a));
    assert!(live.contains(&out));
    assert!(live.contains(&p));
    assert_eq!(live.len(), 3);
}

#[test]
fn live_set_pin_overlaps_dag() {
    let dag = temp_dag();
    let a = leaf(1);
    let r = make_recipe("f", vec![a]);
    let out = dag.insert(&r).unwrap();
    let mut pins = PinSet::new();
    pins.pin(a); // pin an addr already in DAG
    let live = compute_live_set(&dag, &pins);
    assert_eq!(live.len(), 2); // a and out, no duplicate
    assert!(live.contains(&a));
    assert!(live.contains(&out));
}

#[test]
fn live_set_multi_input_recipe() {
    let dag = temp_dag();
    let a = leaf(1);
    let b = leaf(2);
    let c = leaf(3);
    let r = make_recipe("f", vec![a, b, c]);
    let out = dag.insert(&r).unwrap();
    let live = compute_live_set(&dag, &PinSet::new());
    assert_eq!(live.len(), 4);
    assert!(live.contains(&a));
    assert!(live.contains(&b));
    assert!(live.contains(&c));
    assert!(live.contains(&out));
}
