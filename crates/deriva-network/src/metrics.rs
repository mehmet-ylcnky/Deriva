use lazy_static::lazy_static;
use prometheus::{
    register_gauge, register_int_counter, register_int_counter_vec,
    Gauge, IntCounter, IntCounterVec,
};

lazy_static! {
    /// Total SWIM probes sent (one per probe round).
    pub static ref SWIM_PROBES_TOTAL: IntCounter = register_int_counter!(
        "deriva_swim_probes_total",
        "Total SWIM probes sent"
    ).unwrap();

    /// SWIM probes that resulted in suspecting the target (no ack).
    pub static ref SWIM_PROBE_FAILURES: IntCounter = register_int_counter!(
        "deriva_swim_probe_failures_total",
        "SWIM probes with no ack (target suspected)"
    ).unwrap();

    /// Current number of alive cluster members (including self).
    pub static ref SWIM_MEMBERS_ALIVE: Gauge = register_gauge!(
        "deriva_swim_members_alive",
        "Alive cluster members"
    ).unwrap();

    /// Current number of suspected cluster members.
    pub static ref SWIM_MEMBERS_SUSPECT: Gauge = register_gauge!(
        "deriva_swim_members_suspect",
        "Suspect cluster members"
    ).unwrap();

    /// Total bytes sent via SWIM gossip (UDP).
    pub static ref SWIM_GOSSIP_BYTES: IntCounter = register_int_counter!(
        "deriva_swim_gossip_bytes_total",
        "Total gossip bytes sent via UDP"
    ).unwrap();

    /// Total SWIM messages received, labeled by type.
    pub static ref SWIM_MESSAGES_RECEIVED: IntCounterVec = register_int_counter_vec!(
        "deriva_swim_messages_received_total",
        "SWIM messages received by type",
        &["msg_type"]
    ).unwrap();
}
