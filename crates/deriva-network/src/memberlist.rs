use std::collections::HashMap;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;
use rand::Rng;

use crate::types::*;

/// Manages the cluster membership list and implements the SWIM state machine.
///
/// Responsibilities:
/// - Track all known members and their states (Alive/Suspect/Dead)
/// - Apply incoming membership updates with incarnation-based conflict resolution
/// - Provide random peer selection for probing
/// - Maintain a piggyback queue for gossip dissemination
/// - Handle incarnation refutation when this node is falsely suspected
pub struct MemberList {
    /// Our own node ID (mutable for incarnation bumps).
    local: NodeId,
    /// All known members keyed by their NodeId.
    members: HashMap<NodeId, Member>,
    /// Pending updates to piggyback on outgoing messages.
    /// Each entry: (update, remaining_retransmit_count).
    pending_updates: Vec<(MemberUpdate, u8)>,
    /// Max retransmit count for each queued update.
    max_retransmit: u8,
}


impl MemberList {
    /// Create a new MemberList with the local node as the only alive member.
    pub fn new(local: NodeId) -> Self {
        let mut members = HashMap::new();
        members.insert(local.clone(), Member {
            id: local.clone(),
            state: MemberState::Alive,
            state_change: Instant::now(),
            metadata: None,
        });
        Self {
            local,
            members,
            pending_updates: Vec::new(),
            max_retransmit: 4,
        }
    }

    /// Return the local node's NodeId.
    pub fn local_id(&self) -> &NodeId {
        &self.local
    }

    /// Count of members NOT in Dead state (includes self).
    pub fn alive_count(&self) -> usize {
        self.members.values()
            .filter(|m| m.state != MemberState::Dead)
            .count()
    }

    /// All alive members excluding self.
    pub fn alive_peers(&self) -> Vec<&Member> {
        self.members.values()
            .filter(|m| m.id != self.local && m.state == MemberState::Alive)
            .collect()
    }

    /// Pick a uniformly random alive peer for probing. Returns None if no peers.
    pub fn random_peer(&self) -> Option<NodeId> {
        let peers = self.alive_peers();
        if peers.is_empty() {
            return None;
        }
        let idx = rand::thread_rng().gen_range(0..peers.len());
        Some(peers[idx].id.clone())
    }


    /// Pick up to `count` random alive peers, excluding `exclude` and self.
    /// Used for indirect probing (PingReq targets).
    pub fn random_peers_excluding(&self, exclude: &NodeId, count: usize) -> Vec<NodeId> {
        let mut candidates: Vec<NodeId> = self.members.values()
            .filter(|m| {
                m.id != self.local
                    && m.id != *exclude
                    && m.state == MemberState::Alive
            })
            .map(|m| m.id.clone())
            .collect();
        candidates.shuffle(&mut rand::thread_rng());
        candidates.truncate(count);
        candidates
    }

    /// Snapshot of all members and their states (for status/debugging/RPC).
    pub fn all_members(&self) -> Vec<(NodeId, MemberState)> {
        self.members.iter()
            .map(|(id, m)| (id.clone(), m.state))
            .collect()
    }

    /// Get metadata for a specific node, if available.
    pub fn get_metadata(&self, node: &NodeId) -> Option<&NodeMetadata> {
        self.members.get(node).and_then(|m| m.metadata.as_ref())
    }

    /// Update the local node's metadata (called by metadata refresh loop).
    pub fn update_local_metadata(&mut self, metadata: NodeMetadata) {
        if let Some(me) = self.members.get_mut(&self.local) {
            me.metadata = Some(metadata);
        }
    }

    /// Get a reference to a member by NodeId.
    pub fn get_member(&self, node: &NodeId) -> Option<&Member> {
        self.members.get(node)
    }

    /// Expose members map for suspicion timeout checks in cleanup loop.
    pub fn members(&self) -> &HashMap<NodeId, Member> {
        &self.members
    }
}


// --- Task 3.2: State Transitions ---

/// State priority for same-incarnation conflict resolution.
/// Higher value = higher priority = overrides lower.
fn state_priority(s: MemberState) -> u8 {
    match s {
        MemberState::Alive => 0,
        MemberState::Suspect => 1,
        MemberState::Dead => 2,
    }
}

impl MemberList {
    /// Apply a membership update received from gossip or direct observation.
    ///
    /// Implements incarnation-based conflict resolution (FR-4.3):
    /// - Higher incarnation always wins regardless of state
    /// - Equal incarnation: higher state priority wins (Dead > Suspect > Alive)
    /// - Lower incarnation: update is silently discarded
    ///
    /// Returns the SwimEvent to emit if a meaningful state transition occurred.
    pub fn apply_update(&mut self, update: &MemberUpdate) -> Option<SwimEvent> {
        // Self-suspicion refutation (FR-2.7): if someone suspects us, refute immediately
        if update.node == self.local
            && update.state != MemberState::Alive
            && update.incarnation <= self.local.incarnation
        {
            // We'll handle refutation externally; here we just ignore the update about us
            return None;
        }

        if let Some(existing) = self.members.get(&update.node) {
            let existing_inc = existing.id.incarnation;
            let existing_state = existing.state;

            // Rule 1: Lower incarnation — discard as stale
            if update.incarnation < existing_inc {
                return None;
            }

            // Rule 2: Same incarnation — state priority decides
            if update.incarnation == existing_inc {
                if state_priority(update.state) <= state_priority(existing_state) {
                    // Update doesn't dominate — discard
                    return None;
                }
            }

            // Rule 3: Higher incarnation OR same incarnation with higher priority — accept
            let old_state = existing_state;


            // Apply the update to existing member
            let member = self.members.get_mut(&update.node).unwrap();
            member.state = update.state;
            member.state_change = Instant::now();
            member.id.incarnation = update.incarnation;
            if let Some(ref meta) = update.metadata {
                member.metadata = Some(meta.clone());
            }

            // Determine event based on state transition
            match (old_state, update.state) {
                (MemberState::Alive, MemberState::Suspect) =>
                    Some(SwimEvent::MemberSuspect(update.node.clone())),
                (MemberState::Alive, MemberState::Dead) =>
                    Some(SwimEvent::MemberLeft(update.node.clone())),
                (MemberState::Suspect, MemberState::Dead) =>
                    Some(SwimEvent::MemberLeft(update.node.clone())),
                (MemberState::Suspect, MemberState::Alive) =>
                    Some(SwimEvent::MemberAlive(update.node.clone())),
                (MemberState::Dead, MemberState::Alive) =>
                    Some(SwimEvent::MemberAlive(update.node.clone())),
                _ => {
                    // Metadata update without state change
                    if update.metadata.is_some() {
                        update.metadata.as_ref().map(|m| {
                            SwimEvent::MetadataUpdated(update.node.clone(), m.clone())
                        })
                    } else {
                        None
                    }
                }
            }
        } else {
            // New member — insert into the member list
            self.members.insert(update.node.clone(), Member {
                id: NodeId {
                    addr: update.node.addr,
                    incarnation: update.incarnation,
                    name: update.node.name.clone(),
                },
                state: update.state,
                state_change: Instant::now(),
                metadata: update.metadata.clone(),
            });

            match update.state {
                MemberState::Alive => Some(SwimEvent::MemberJoined(update.node.clone())),
                MemberState::Suspect => Some(SwimEvent::MemberSuspect(update.node.clone())),
                MemberState::Dead => None, // Don't emit join for already-dead nodes
            }
        }
    }


    /// Convenience: mark a node as SUSPECT (failed probe).
    /// Uses the node's current incarnation from our records.
    pub fn suspect(&mut self, node: &NodeId) -> Option<SwimEvent> {
        let incarnation = self.members.get(node)
            .map(|m| m.id.incarnation)
            .unwrap_or(node.incarnation);
        self.apply_update(&MemberUpdate {
            node: node.clone(),
            state: MemberState::Suspect,
            incarnation,
            metadata: None,
        })
    }

    /// Convenience: mark a node as DEAD (suspicion timeout expired).
    /// Uses the node's current incarnation from our records.
    pub fn declare_dead(&mut self, node: &NodeId) -> Option<SwimEvent> {
        let incarnation = self.members.get(node)
            .map(|m| m.id.incarnation)
            .unwrap_or(node.incarnation);
        self.apply_update(&MemberUpdate {
            node: node.clone(),
            state: MemberState::Dead,
            incarnation,
            metadata: None,
        })
    }

    /// Refute suspicion about ourselves (FR-10.1–FR-10.3).
    ///
    /// Increments our incarnation number so that any node holding
    /// a suspect/dead update about us at a lower incarnation will
    /// accept our alive update and clear the suspicion.
    ///
    /// Returns the MemberUpdate to queue for piggybacking.
    pub fn refute(&mut self) -> MemberUpdate {
        self.local.next_incarnation();
        if let Some(me) = self.members.get_mut(&self.local) {
            me.id.incarnation = self.local.incarnation;
            me.state = MemberState::Alive;
            me.state_change = Instant::now();
        }
        MemberUpdate {
            node: self.local.clone(),
            state: MemberState::Alive,
            incarnation: self.local.incarnation,
            metadata: self.members.get(&self.local)
                .and_then(|m| m.metadata.clone()),
        }
    }
}


// --- Task 3.3: Gossip Queue ---

impl MemberList {
    /// Queue a membership update for piggybacking on future outgoing messages.
    ///
    /// The update will be attached to up to `max_retransmit` (default 4) outgoing
    /// messages before being dropped from the queue. This ensures updates propagate
    /// even if some messages are lost.
    pub fn queue_update(&mut self, update: MemberUpdate) {
        // Avoid duplicates: remove existing update for same node if present
        self.pending_updates.retain(|(u, _)| u.node != update.node);
        self.pending_updates.push((update, self.max_retransmit));
    }

    /// Drain up to `max_count` pending updates for piggybacking on an outgoing message.
    ///
    /// Updates are taken from the front (most recent first per FR-4.5).
    /// Each returned update has its retransmit counter decremented.
    /// Updates whose counter reaches 0 are removed from the queue.
    pub fn drain_piggyback(&mut self, max_count: usize) -> Vec<MemberUpdate> {
        let mut result = Vec::with_capacity(max_count);
        let mut i = 0;
        while i < self.pending_updates.len() && result.len() < max_count {
            result.push(self.pending_updates[i].0.clone());
            self.pending_updates[i].1 = self.pending_updates[i].1.saturating_sub(1);
            if self.pending_updates[i].1 == 0 {
                self.pending_updates.remove(i);
            } else {
                i += 1;
            }
        }
        result
    }

    /// Remove members that have been in DEAD state longer than `timeout`.
    ///
    /// Returns the list of NodeIds that were purged (FR-2.4).
    /// This prevents the member list from growing unbounded with long-dead nodes.
    pub fn cleanup_dead(&mut self, timeout: Duration) -> Vec<NodeId> {
        let now = Instant::now();
        let mut removed = Vec::new();
        self.members.retain(|id, member| {
            if member.state == MemberState::Dead
                && now.duration_since(member.state_change) > timeout
                && *id != self.local  // Never remove self
            {
                removed.push(id.clone());
                false
            } else {
                true
            }
        });
        // Also remove pending updates for purged nodes
        let removed_set: std::collections::HashSet<&NodeId> = removed.iter().collect();
        self.pending_updates.retain(|(u, _)| !removed_set.contains(&u.node));
        removed
    }

    /// Check if a node is suspected about us (used by runtime to trigger refutation).
    pub fn is_self_suspected(&self) -> bool {
        self.members.get(&self.local)
            .map(|m| m.state == MemberState::Suspect)
            .unwrap_or(false)
    }

    /// Get the number of pending piggyback updates.
    pub fn pending_count(&self) -> usize {
        self.pending_updates.len()
    }
}
