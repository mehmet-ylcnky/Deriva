use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock, watch};

use crate::config::SwimConfig;
use crate::memberlist::MemberList;
use crate::message::SwimMessage;
use crate::types::*;

/// The main SWIM runtime. Manages the probe loop, gossip dissemination,
/// failure detection, and metadata exchange.
///
/// Spawns background tokio tasks for:
/// 1. Receive loop — handle incoming UDP messages
/// 2. Probe loop — periodic ping of random member + indirect probes
/// 3. Cleanup loop — promote suspects to dead, purge long-dead members
pub struct SwimRuntime {
    config: SwimConfig,
    local_id: NodeId,
    members: Arc<RwLock<MemberList>>,
    socket: Arc<UdpSocket>,
    event_tx: mpsc::Sender<SwimEvent>,
    sequence: Arc<AtomicU64>,
    shutdown_tx: watch::Sender<bool>,
}


// --- 4.1: Constructor and seed joining ---

impl SwimRuntime {
    /// Create and start the SWIM runtime.
    ///
    /// Binds a UDP socket, joins seed nodes, and spawns background tasks.
    /// Returns (runtime_handle, event_receiver).
    pub async fn start(
        config: SwimConfig,
    ) -> Result<(Self, mpsc::Receiver<SwimEvent>), std::io::Error> {
        let socket = UdpSocket::bind(&config.bind_addr).await?;
        let actual_addr = socket.local_addr()?;
        let socket = Arc::new(socket);

        let local_id = NodeId::new(
            actual_addr,
            config.node_name.clone().unwrap_or_else(|| {
                format!("node-{}", actual_addr.port())
            }),
        );

        let members = Arc::new(RwLock::new(MemberList::new(local_id.clone())));
        let (event_tx, event_rx) = mpsc::channel(256);
        let (shutdown_tx, _) = watch::channel(false);

        let runtime = Self {
            config: config.clone(),
            local_id: local_id.clone(),
            members: members.clone(),
            socket: socket.clone(),
            event_tx: event_tx.clone(),
            sequence: Arc::new(AtomicU64::new(0)),
            shutdown_tx,
        };

        // Join seed nodes by sending initial Ping
        runtime.join_seeds().await;

        // Spawn background tasks
        runtime.spawn_receive_loop();
        runtime.spawn_probe_loop();
        runtime.spawn_cleanup_loop();

        Ok((runtime, event_rx))
    }

    /// Send Ping to each seed address (skip self per FR-9.2).
    async fn join_seeds(&self) {
        for seed_addr in &self.config.seeds {
            if *seed_addr == self.local_id.addr {
                continue;
            }
            let seq = self.next_seq();
            let msg = SwimMessage::Ping {
                sender: self.local_id.clone(),
                sequence: seq,
                piggyback: vec![],
            };
            if let Ok(data) = msg.encode() {
                let _ = self.socket.send_to(&data, seed_addr).await;
            }
        }
    }

    fn next_seq(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }
}


// --- 4.2: Receive loop ---

impl SwimRuntime {
    fn spawn_receive_loop(&self) {
        let socket = self.socket.clone();
        let members = self.members.clone();
        let event_tx = self.event_tx.clone();
        let local_id = self.local_id.clone();
        let config = self.config.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => break,
                    result = socket.recv_from(&mut buf) => {
                        let (len, src) = match result {
                            Ok(r) => r,
                            Err(_) => continue,
                        };
                        let msg = match SwimMessage::decode(&buf[..len]) {
                            Ok(m) => m,
                            Err(_) => {
                                tracing::debug!("dropped malformed message from {}", src);
                                continue;
                            }
                        };
                        Self::handle_message(
                            &msg, src, &socket, &members,
                            &event_tx, &local_id, &config,
                        ).await;
                    }
                }
            }
        });
    }

    async fn handle_message(
        msg: &SwimMessage,
        _src: std::net::SocketAddr,
        socket: &UdpSocket,
        members: &RwLock<MemberList>,
        event_tx: &mpsc::Sender<SwimEvent>,
        local_id: &NodeId,
        config: &SwimConfig,
    ) {
        // Process piggybacked updates from any message type
        {
            let mut ml = members.write().await;
            for update in msg.piggyback() {
                if let Some(event) = ml.apply_update(update) {
                    let _ = event_tx.try_send(event);
                }
            }
            // If we've been suspected, refute immediately
            if ml.is_self_suspected() {
                let refutation = ml.refute();
                ml.queue_update(refutation);
            }
        }


        match msg {
            SwimMessage::Ping { sender, sequence, .. } => {
                // Mark sender as alive
                {
                    let mut ml = members.write().await;
                    ml.apply_update(&MemberUpdate {
                        node: sender.clone(),
                        state: MemberState::Alive,
                        incarnation: sender.incarnation,
                        metadata: None,
                    });
                }
                // Respond with Ack + piggybacked updates
                let piggyback = {
                    let mut ml = members.write().await;
                    ml.drain_piggyback(config.max_piggyback)
                };
                let ack = SwimMessage::Ack {
                    sender: local_id.clone(),
                    sequence: *sequence,
                    piggyback,
                };
                if let Ok(data) = ack.encode() {
                    let _ = socket.send_to(&data, sender.addr).await;
                }
            }

            SwimMessage::Ack { sender, .. } => {
                // Mark sender as alive
                let mut ml = members.write().await;
                if let Some(event) = ml.apply_update(&MemberUpdate {
                    node: sender.clone(),
                    state: MemberState::Alive,
                    incarnation: sender.incarnation,
                    metadata: None,
                }) {
                    let _ = event_tx.try_send(event);
                }
            }

            SwimMessage::PingReq { target, sender, sequence, .. } => {
                // Forward Ping to target on behalf of original sender
                let piggyback = {
                    let mut ml = members.write().await;
                    ml.drain_piggyback(config.max_piggyback)
                };
                let ping = SwimMessage::Ping {
                    sender: local_id.clone(),
                    sequence: *sequence,
                    piggyback,
                };
                if let Ok(data) = ping.encode() {
                    let _ = socket.send_to(&data, target.addr).await;
                }
                // Also mark the PingReq sender as alive
                let mut ml = members.write().await;
                ml.apply_update(&MemberUpdate {
                    node: sender.clone(),
                    state: MemberState::Alive,
                    incarnation: sender.incarnation,
                    metadata: None,
                });
            }

            SwimMessage::Sync { members: remote_members, sender, .. } => {
                let mut ml = members.write().await;
                // Mark sync sender as alive
                ml.apply_update(&MemberUpdate {
                    node: sender.clone(),
                    state: MemberState::Alive,
                    incarnation: sender.incarnation,
                    metadata: None,
                });
                for update in remote_members {
                    if let Some(event) = ml.apply_update(update) {
                        let _ = event_tx.try_send(event);
                    }
                }
            }
        }
    }
}


// --- 4.3: Probe loop ---

impl SwimRuntime {
    fn spawn_probe_loop(&self) {
        let socket = self.socket.clone();
        let members = self.members.clone();
        let event_tx = self.event_tx.clone();
        let local_id = self.local_id.clone();
        let config = self.config.clone();
        let sequence = self.sequence.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.probe_interval);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => break,
                    _ = interval.tick() => {
                        Self::probe_cycle(
                            &socket, &members, &event_tx,
                            &local_id, &config, &sequence,
                        ).await;
                    }
                }
            }
        });
    }

    /// Execute one full probe cycle:
    /// 1. Pick random alive peer
    /// 2. Send direct Ping, wait for Ack
    /// 3. If no Ack: send indirect PingReq via K peers, wait again
    /// 4. If still no Ack: mark target as SUSPECT
    async fn probe_cycle(
        socket: &UdpSocket,
        members: &RwLock<MemberList>,
        event_tx: &mpsc::Sender<SwimEvent>,
        local_id: &NodeId,
        config: &SwimConfig,
        sequence: &AtomicU64,
    ) {
        // Step 1: Pick random peer
        let target = {
            let ml = members.read().await;
            ml.random_peer()
        };
        let target = match target {
            Some(t) => t,
            None => return, // No peers — single-node mode (FR-3.7)
        };

        let seq = sequence.fetch_add(1, Ordering::Relaxed);

        // Step 2: Send direct Ping
        let piggyback = {
            let mut ml = members.write().await;
            ml.drain_piggyback(config.max_piggyback)
        };
        let ping = SwimMessage::Ping {
            sender: local_id.clone(),
            sequence: seq,
            piggyback,
        };
        if let Ok(data) = ping.encode() {
            let _ = socket.send_to(&data, target.addr).await;
        }


        // Wait for direct Ack (probe_timeout)
        tokio::time::sleep(config.probe_timeout).await;

        // Check if target is now alive (Ack was processed by receive loop)
        let target_alive = {
            let ml = members.read().await;
            ml.get_member(&target)
                .map(|m| m.state == MemberState::Alive)
                .unwrap_or(false)
        };
        if target_alive {
            return; // Direct probe succeeded
        }

        // Step 3: Indirect probes via K random peers
        let relay_peers = {
            let ml = members.read().await;
            ml.random_peers_excluding(&target, config.indirect_probes)
        };

        for relay in &relay_peers {
            let piggyback = {
                let mut ml = members.write().await;
                ml.drain_piggyback(config.max_piggyback)
            };
            let ping_req = SwimMessage::PingReq {
                sender: local_id.clone(),
                target: target.clone(),
                sequence: seq,
                piggyback,
            };
            if let Ok(data) = ping_req.encode() {
                let _ = socket.send_to(&data, relay.addr).await;
            }
        }

        // Wait for indirect Ack (another probe_timeout)
        tokio::time::sleep(config.probe_timeout).await;

        // Step 4: Check again — if still not alive, mark as suspect
        let still_not_alive = {
            let ml = members.read().await;
            ml.get_member(&target)
                .map(|m| m.state != MemberState::Alive)
                .unwrap_or(true)
        };

        if still_not_alive {
            let mut ml = members.write().await;
            if let Some(event) = ml.suspect(&target) {
                // Queue the suspect update for gossip dissemination
                ml.queue_update(MemberUpdate {
                    node: target.clone(),
                    state: MemberState::Suspect,
                    incarnation: target.incarnation,
                    metadata: None,
                });
                let _ = event_tx.try_send(event);
            }
        }
    }
}


// --- 4.4: Cleanup loop ---

impl SwimRuntime {
    fn spawn_cleanup_loop(&self) {
        let members = self.members.clone();
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Run cleanup at half the dead_cleanup interval for responsiveness
        let cleanup_interval = config.dead_cleanup / 2;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_rx.changed() => break,
                    _ = interval.tick() => {
                        let mut ml = members.write().await;

                        // Compute suspicion timeout based on current cluster size
                        let n = ml.alive_count();
                        let suspicion_timeout = config.suspicion_timeout(n);

                        // Promote suspects past suspicion timeout to DEAD
                        let suspects: Vec<NodeId> = ml.members().iter()
                            .filter(|(_, m)| m.state == MemberState::Suspect)
                            .filter(|(_, m)| m.state_change.elapsed() > suspicion_timeout)
                            .map(|(id, _)| id.clone())
                            .collect();

                        for suspect_id in suspects {
                            if let Some(event) = ml.declare_dead(&suspect_id) {
                                ml.queue_update(MemberUpdate {
                                    node: suspect_id,
                                    state: MemberState::Dead,
                                    incarnation: 0, // Will use recorded incarnation
                                    metadata: None,
                                });
                                let _ = event_tx.try_send(event);
                            }
                        }

                        // Purge long-dead members from the list entirely
                        ml.cleanup_dead(config.dead_cleanup);
                    }
                }
            }
        });
    }
}


// --- 4.5: Public API ---

impl SwimRuntime {
    /// Get a snapshot of all members and their states.
    pub async fn members(&self) -> Vec<(NodeId, MemberState)> {
        let ml = self.members.read().await;
        ml.all_members()
    }

    /// Get the number of alive members (including self).
    pub async fn alive_count(&self) -> usize {
        let ml = self.members.read().await;
        ml.alive_count()
    }

    /// Get metadata for a specific node.
    pub async fn get_metadata(&self, node: &NodeId) -> Option<NodeMetadata> {
        let ml = self.members.read().await;
        ml.get_metadata(node).cloned()
    }

    /// Update local node's metadata (called by metadata refresh task).
    pub async fn update_metadata(&self, metadata: NodeMetadata) {
        let mut ml = self.members.write().await;
        ml.update_local_metadata(metadata);
    }

    /// Get the local node ID.
    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    /// Get the config used by this runtime.
    pub fn config(&self) -> &SwimConfig {
        &self.config
    }

    /// Gracefully shut down the SWIM runtime.
    /// All background tasks will terminate on the next loop iteration.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Get access to the internal member list (for testing/advanced use).
    pub fn member_list(&self) -> &Arc<RwLock<MemberList>> {
        &self.members
    }
}
