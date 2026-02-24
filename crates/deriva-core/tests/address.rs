use deriva_core::address::*;
use std::collections::BTreeMap;

// --- Test Group 1: CAddr basics ---

#[test]
fn caddr_from_bytes_deterministic() {
    let a1 = CAddr::from_bytes(b"hello world");
    let a2 = CAddr::from_bytes(b"hello world");
    assert_eq!(a1, a2);
}

#[test]
fn caddr_different_bytes_different_addr() {
    assert_ne!(CAddr::from_bytes(b"hello"), CAddr::from_bytes(b"world"));
}

#[test]
fn caddr_empty_bytes() {
    let a = CAddr::from_bytes(b"");
    assert_eq!(a.as_bytes().len(), 32);
    assert_eq!(a, CAddr::from_bytes(b""));
}

#[test]
fn caddr_hex_encoding() {
    let hex = CAddr::from_bytes(b"test").to_hex();
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn caddr_debug_truncates() {
    let debug = format!("{:?}", CAddr::from_bytes(b"test"));
    assert!(debug.starts_with("CAddr("));
    assert!(debug.contains('…'));
    assert!(debug.len() < 30);
}

#[test]
fn caddr_display_full_hex() {
    assert_eq!(format!("{}", CAddr::from_bytes(b"test")).len(), 64);
}

#[test]
fn caddr_from_raw_roundtrip() {
    let addr = CAddr::from_bytes(b"roundtrip");
    assert_eq!(addr, CAddr::from_raw(*addr.as_bytes()));
}

#[test]
fn caddr_hash_usable_in_hashmap() {
    let mut map = std::collections::HashMap::new();
    let addr = CAddr::from_bytes(b"key");
    map.insert(addr, "value");
    assert_eq!(map.get(&addr), Some(&"value"));
}

#[test]
fn caddr_ord_is_consistent() {
    let a = CAddr::from_bytes(b"aaa");
    let b = CAddr::from_bytes(b"bbb");
    assert_eq!(a.cmp(&b), a.cmp(&b));
}

// --- Test Group 2: FunctionId ---

#[test]
fn function_id_equality() {
    assert_eq!(FunctionId::new("compress", "1.0.0"), FunctionId::new("compress", "1.0.0"));
}

#[test]
fn function_id_different_version() {
    assert_ne!(FunctionId::new("compress", "1.0.0"), FunctionId::new("compress", "2.0.0"));
}

#[test]
fn function_id_different_name() {
    assert_ne!(FunctionId::new("compress", "1.0.0"), FunctionId::new("decompress", "1.0.0"));
}

#[test]
fn function_id_display() {
    assert_eq!(format!("{}", FunctionId::new("compress", "1.0.0")), "compress/1.0.0");
}

#[test]
fn function_id_serialization_roundtrip() {
    let fid = FunctionId::new("transform", "3.2.1");
    let bytes = bincode::serialize(&fid).unwrap();
    let restored: FunctionId = bincode::deserialize(&bytes).unwrap();
    assert_eq!(fid, restored);
}

// --- Test Group 3: Value enum ---

#[test]
fn value_string() {
    let v = Value::String("hello".into());
    let restored: Value = bincode::deserialize(&bincode::serialize(&v).unwrap()).unwrap();
    assert_eq!(v, restored);
}

#[test]
fn value_int() {
    let v = Value::Int(-42);
    let restored: Value = bincode::deserialize(&bincode::serialize(&v).unwrap()).unwrap();
    assert_eq!(v, restored);
}

#[test]
fn value_bool() {
    let v = Value::Bool(true);
    let restored: Value = bincode::deserialize(&bincode::serialize(&v).unwrap()).unwrap();
    assert_eq!(v, restored);
}

#[test]
fn value_bytes() {
    let v = Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
    let restored: Value = bincode::deserialize(&bincode::serialize(&v).unwrap()).unwrap();
    assert_eq!(v, restored);
}

#[test]
fn value_list_nested() {
    let v = Value::List(vec![
        Value::Int(1),
        Value::String("two".into()),
        Value::List(vec![Value::Bool(false)]),
    ]);
    let restored: Value = bincode::deserialize(&bincode::serialize(&v).unwrap()).unwrap();
    assert_eq!(v, restored);
}

#[test]
fn value_ordering_is_deterministic() {
    assert!(Value::Int(1) < Value::Int(2));
}

// --- Test Group 4: Recipe canonical serialization ---

#[test]
fn recipe_canonical_bytes_deterministic() {
    let recipe = Recipe::new(
        FunctionId::new("compress", "1.0.0"),
        vec![CAddr::from_bytes(b"input1")],
        BTreeMap::from([("algo".into(), Value::String("zstd".into()))]),
    );
    assert_eq!(recipe.canonical_bytes(), recipe.canonical_bytes());
}

#[test]
fn recipe_addr_deterministic() {
    let recipe = Recipe::new(
        FunctionId::new("compress", "1.0.0"),
        vec![CAddr::from_bytes(b"input1")],
        BTreeMap::from([("algo".into(), Value::String("zstd".into()))]),
    );
    assert_eq!(recipe.addr(), recipe.addr());
}

#[test]
fn recipe_different_function_different_addr() {
    let inputs = vec![CAddr::from_bytes(b"data")];
    let r1 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs.clone(), BTreeMap::new());
    let r2 = Recipe::new(FunctionId::new("decompress", "1.0.0"), inputs, BTreeMap::new());
    assert_ne!(r1.addr(), r2.addr());
}

#[test]
fn recipe_different_version_different_addr() {
    let inputs = vec![CAddr::from_bytes(b"data")];
    let r1 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs.clone(), BTreeMap::new());
    let r2 = Recipe::new(FunctionId::new("compress", "2.0.0"), inputs, BTreeMap::new());
    assert_ne!(r1.addr(), r2.addr());
}

#[test]
fn recipe_different_inputs_different_addr() {
    let r1 = Recipe::new(FunctionId::new("concat", "1.0.0"), vec![CAddr::from_bytes(b"a")], BTreeMap::new());
    let r2 = Recipe::new(FunctionId::new("concat", "1.0.0"), vec![CAddr::from_bytes(b"b")], BTreeMap::new());
    assert_ne!(r1.addr(), r2.addr());
}

#[test]
fn recipe_input_order_matters() {
    let a = CAddr::from_bytes(b"first");
    let b = CAddr::from_bytes(b"second");
    let r1 = Recipe::new(FunctionId::new("concat", "1.0.0"), vec![a, b], BTreeMap::new());
    let r2 = Recipe::new(FunctionId::new("concat", "1.0.0"), vec![b, a], BTreeMap::new());
    assert_ne!(r1.addr(), r2.addr());
}

#[test]
fn recipe_different_params_different_addr() {
    let inputs = vec![CAddr::from_bytes(b"data")];
    let r1 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs.clone(), BTreeMap::from([("level".into(), Value::Int(3))]));
    let r2 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs, BTreeMap::from([("level".into(), Value::Int(9))]));
    assert_ne!(r1.addr(), r2.addr());
}

#[test]
fn recipe_param_key_order_irrelevant() {
    let inputs = vec![CAddr::from_bytes(b"data")];
    let mut p1 = BTreeMap::new();
    p1.insert("algo".into(), Value::String("zstd".into()));
    p1.insert("level".into(), Value::Int(3));
    let mut p2 = BTreeMap::new();
    p2.insert("level".into(), Value::Int(3));
    p2.insert("algo".into(), Value::String("zstd".into()));
    let r1 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs.clone(), p1);
    let r2 = Recipe::new(FunctionId::new("compress", "1.0.0"), inputs, p2);
    assert_eq!(r1.addr(), r2.addr());
}

#[test]
fn recipe_empty_inputs_and_params() {
    let recipe = Recipe::new(FunctionId::new("generate", "1.0.0"), vec![], BTreeMap::new());
    assert_eq!(recipe.addr().as_bytes().len(), 32);
}

#[test]
fn recipe_serialization_roundtrip() {
    let recipe = Recipe::new(
        FunctionId::new("transform", "2.1.0"),
        vec![CAddr::from_bytes(b"in1"), CAddr::from_bytes(b"in2")],
        BTreeMap::from([
            ("mode".into(), Value::String("fast".into())),
            ("threads".into(), Value::Int(4)),
        ]),
    );
    let bytes = recipe.canonical_bytes();
    let restored: Recipe = bincode::deserialize(&bytes).unwrap();
    assert_eq!(recipe, restored);
    assert_eq!(recipe.addr(), restored.addr());
}

#[test]
fn recipe_many_inputs() {
    let inputs: Vec<CAddr> = (0..100).map(|i| CAddr::from_bytes(format!("input_{i}").as_bytes())).collect();
    let recipe = Recipe::new(FunctionId::new("merge", "1.0.0"), inputs, BTreeMap::new());
    assert_eq!(recipe.addr().as_bytes().len(), 32);
}

// --- Test Group 5: DataRef ---

#[test]
fn dataref_leaf_addr_matches_content_hash() {
    let data = b"raw sensor data";
    let dref = DataRef::leaf(data);
    assert_eq!(*dref.addr(), CAddr::from_bytes(data));
    if let DataRef::Leaf { size, .. } = dref {
        assert_eq!(size, data.len() as u64);
    } else {
        panic!("expected Leaf");
    }
}

#[test]
fn dataref_derived_addr_matches_recipe_addr() {
    let recipe = Recipe::new(FunctionId::new("compress", "1.0.0"), vec![CAddr::from_bytes(b"input")], BTreeMap::new());
    let expected = recipe.addr();
    assert_eq!(*DataRef::derived(recipe).addr(), expected);
}

#[test]
fn dataref_leaf_and_derived_have_different_addrs() {
    let data = b"some data";
    let leaf = DataRef::leaf(data);
    let derived = DataRef::derived(Recipe::new(
        FunctionId::new("identity", "1.0.0"),
        vec![CAddr::from_bytes(data)],
        BTreeMap::new(),
    ));
    assert_ne!(leaf.addr(), derived.addr());
}

#[test]
fn dataref_serialization_roundtrip_leaf() {
    let dref = DataRef::leaf(b"test data");
    let restored: DataRef = bincode::deserialize(&bincode::serialize(&dref).unwrap()).unwrap();
    assert_eq!(dref, restored);
}

#[test]
fn dataref_serialization_roundtrip_derived() {
    let dref = DataRef::derived(Recipe::new(
        FunctionId::new("fn", "1.0.0"),
        vec![CAddr::from_bytes(b"x")],
        BTreeMap::from([("k".into(), Value::Bool(true))]),
    ));
    let restored: DataRef = bincode::deserialize(&bincode::serialize(&dref).unwrap()).unwrap();
    assert_eq!(dref, restored);
}
