use std::collections::HashSet;
use deriva_core::CAddr;
use deriva_core::gc::PinSet;
use deriva_core::persistent_dag::PersistentDag;

/// Compute the set of live CAddrs that must not be garbage collected.
pub fn compute_live_set(dag: &PersistentDag, pins: &PinSet) -> HashSet<CAddr> {
    let mut live = dag.live_addr_set();
    for addr in pins.as_set() {
        live.insert(*addr);
    }
    live
}
