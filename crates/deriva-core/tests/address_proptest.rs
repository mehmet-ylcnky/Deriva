use deriva_core::address::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<String>().prop_map(Value::String),
        any::<i64>().prop_map(Value::Int),
        any::<bool>().prop_map(Value::Bool),
        prop::collection::vec(any::<u8>(), 0..64).prop_map(Value::Bytes),
    ]
}

fn arb_function_id() -> impl Strategy<Value = FunctionId> {
    ("[a-z_]{1,20}", "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}")
        .prop_map(|(name, version)| FunctionId::new(name, version))
}

fn arb_caddr() -> impl Strategy<Value = CAddr> {
    prop::collection::vec(any::<u8>(), 1..256)
        .prop_map(|bytes| CAddr::from_bytes(&bytes))
}

fn arb_params() -> impl Strategy<Value = BTreeMap<String, Value>> {
    prop::collection::btree_map("[a-z_]{1,10}", arb_value(), 0..8)
}

fn arb_recipe() -> impl Strategy<Value = Recipe> {
    (arb_function_id(), prop::collection::vec(arb_caddr(), 0..10), arb_params())
        .prop_map(|(fid, inputs, params)| Recipe::new(fid, inputs, params))
}

proptest! {
    #[test]
    fn canonical_bytes_are_deterministic(recipe in arb_recipe()) {
        prop_assert_eq!(recipe.canonical_bytes(), recipe.canonical_bytes());
    }

    #[test]
    fn addr_is_deterministic(recipe in arb_recipe()) {
        prop_assert_eq!(recipe.addr(), recipe.addr());
    }

    #[test]
    fn recipe_roundtrip_preserves_addr(recipe in arb_recipe()) {
        let restored: Recipe = bincode::deserialize(&recipe.canonical_bytes()).unwrap();
        prop_assert_eq!(recipe.addr(), restored.addr());
        prop_assert_eq!(&recipe, &restored);
    }

    #[test]
    fn clone_preserves_addr(recipe in arb_recipe()) {
        prop_assert_eq!(recipe.addr(), recipe.clone().addr());
    }

    #[test]
    fn caddr_from_bytes_deterministic(data in prop::collection::vec(any::<u8>(), 0..1024)) {
        prop_assert_eq!(CAddr::from_bytes(&data), CAddr::from_bytes(&data));
    }

    #[test]
    fn caddr_hex_is_valid(data in prop::collection::vec(any::<u8>(), 1..256)) {
        let hex = CAddr::from_bytes(&data).to_hex();
        prop_assert_eq!(hex.len(), 64);
        prop_assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn dataref_leaf_addr_consistent(data in prop::collection::vec(any::<u8>(), 0..512)) {
        prop_assert_eq!(*DataRef::leaf(&data).addr(), CAddr::from_bytes(&data));
    }

    #[test]
    fn dataref_derived_addr_consistent(recipe in arb_recipe()) {
        let expected = recipe.addr();
        prop_assert_eq!(*DataRef::derived(recipe).addr(), expected);
    }
}
