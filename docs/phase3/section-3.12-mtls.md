# §3.12 Mutual TLS & Node Authentication

> **Status:** Not started
> **Depends on:** §3.1 (SWIM gossip), §3.8 (cluster bootstrap)
> **Crate(s):** `deriva-network`, `deriva-server`
> **Estimated effort:** 2–3 days

---

## 1. Problem Statement

Without transport encryption and authentication, a Deriva cluster is vulnerable to:

1. **Eavesdropping**: inter-node traffic (blob data, recipes, GC commands) travels
   in plaintext. Any network observer can read all data.

2. **Impersonation**: a rogue node can join the cluster, receive replicated data,
   issue GC sweeps to delete blobs, or inject poisoned recipes.

3. **Man-in-the-middle**: an attacker intercepts node-to-node traffic, modifies
   blob content in transit. Content-addressing catches data corruption but not
   metadata manipulation (e.g., routing a FetchValue to a malicious node).

4. **Unauthorized client access**: without TLS, any client on the network can
   read/write data without authentication.

### Goals

- **Mutual TLS (mTLS)** on all inter-node gRPC channels: both sides present
  certificates, both verify against a shared CA.
- **X.509 certificates** with cluster-scoped CA: each node gets a cert signed
  by the cluster CA. Nodes reject connections from certs signed by other CAs.
- **Certificate hot-reload**: rotate certs without restarting nodes. File watcher
  detects cert changes and reloads within 30 seconds.
- **Client-to-server TLS**: optional, one-way TLS (server presents cert, client
  verifies). mTLS for clients is opt-in.
- **Graceful degradation**: if TLS is misconfigured, node refuses to start rather
  than running insecure.

### Non-Goals

- Authorization (RBAC, per-operation permissions) — future §4.x.
- Certificate issuance / built-in CA — operators provide certs.
- ACME / Let's Encrypt integration.

---

## 2. Design

### 2.1 Architecture Overview

```
  ┌─────────────────────────────────────────────────────────┐
  │                    Cluster CA                           │
  │  (operator-managed, signs all node certs)               │
  │  ca.crt                                                 │
  └────────┬──────────────────┬──────────────────┬──────────┘
           │                  │                  │
     ┌─────▼─────┐     ┌─────▼─────┐     ┌─────▼─────┐
     │  Node A    │     │  Node B    │     │  Node C    │
     │ node-a.crt │     │ node-b.crt │     │ node-c.crt │
     │ node-a.key │     │ node-b.key │     │ node-c.key │
     └─────┬─────┘     └─────┬─────┘     └─────┬─────┘
           │                  │                  │
           │◄── mTLS ────────►│◄── mTLS ────────►│
           │  (both verify    │  (both verify    │
           │   against CA)    │   against CA)    │

  Client ──── TLS (optional) ────► Node A
              (server cert only,
               or mTLS if configured)
```

### 2.2 Certificate Layout on Disk

```
<config_dir>/tls/
├── ca.crt              # CA certificate (PEM)
├── node.crt            # This node's certificate (PEM)
├── node.key            # This node's private key (PEM)
└── client/             # (optional) client-facing certs
    ├── server.crt      # Server cert for client connections
    └── server.key      # Server key for client connections
```

### 2.3 Core Types

```rust
/// TLS configuration for a Deriva node.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Enable mTLS for inter-node communication.
    pub inter_node_mtls: bool,          // default: true
    /// Enable TLS for client-facing gRPC.
    pub client_tls: bool,               // default: false
    /// Require client certificates (mTLS for clients).
    pub client_mtls: bool,              // default: false
    /// Path to CA certificate.
    pub ca_cert_path: PathBuf,
    /// Path to this node's certificate.
    pub node_cert_path: PathBuf,
    /// Path to this node's private key.
    pub node_key_path: PathBuf,
    /// Optional: separate certs for client-facing port.
    pub client_cert_path: Option<PathBuf>,
    pub client_key_path: Option<PathBuf>,
    /// Enable certificate hot-reload via file watcher.
    pub hot_reload: bool,               // default: true
    /// Hot-reload check interval.
    pub reload_interval: Duration,      // default: 30s
    /// Minimum TLS version.
    pub min_tls_version: TlsVersion,    // default: TLS 1.3
    /// Allowed cipher suites (empty = rustls defaults).
    pub cipher_suites: Vec<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            inter_node_mtls: true,
            client_tls: false,
            client_mtls: false,
            ca_cert_path: PathBuf::from("tls/ca.crt"),
            node_cert_path: PathBuf::from("tls/node.crt"),
            node_key_path: PathBuf::from("tls/node.key"),
            client_cert_path: None,
            client_key_path: None,
            hot_reload: true,
            reload_interval: Duration::from_secs(30),
            min_tls_version: TlsVersion::TLS13,
            cipher_suites: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TlsVersion {
    TLS12,
    TLS13,
}

/// Manages TLS state, cert loading, and hot-reload.
pub struct TlsManager {
    config: TlsConfig,
    /// Current server TLS config (for accepting connections).
    server_config: Arc<ArcSwap<rustls::ServerConfig>>,
    /// Current client TLS config (for outgoing connections).
    client_config: Arc<ArcSwap<rustls::ClientConfig>>,
    /// File modification times for change detection.
    cert_mtimes: Mutex<CertMtimes>,
    cancel: CancellationToken,
}

#[derive(Debug, Default)]
struct CertMtimes {
    ca: Option<SystemTime>,
    cert: Option<SystemTime>,
    key: Option<SystemTime>,
}
```

### 2.4 Certificate Validation Rules

```
Inter-node mTLS:
  Server side:
    - Present node.crt signed by CA
    - Require client cert signed by same CA
    - Verify client cert not expired
    - Verify client cert not revoked (if CRL provided)

  Client side (outgoing to peer):
    - Present node.crt as client cert
    - Verify server cert signed by CA
    - Verify server cert not expired

  SAN (Subject Alternative Name) check:
    - Node cert should have SAN matching node's hostname or IP
    - For internal cluster: IP SAN preferred
    - Wildcard: *.deriva.internal (optional)

Client-to-server TLS:
  Server side:
    - Present server.crt (or node.crt if no separate client cert)
    - If client_mtls=true: require client cert signed by CA
    - If client_mtls=false: no client cert required

  Client side:
    - Verify server cert signed by CA (or system trust store)
```

---

## 3. Implementation

### 3.1 TlsManager

```rust
impl TlsManager {
    pub fn new(config: TlsConfig) -> Result<Self, DerivaError> {
        let (server_cfg, client_cfg) = Self::load_configs(&config)?;

        let mgr = Self {
            config,
            server_config: Arc::new(ArcSwap::from_pointee(server_cfg)),
            client_config: Arc::new(ArcSwap::from_pointee(client_cfg)),
            cert_mtimes: Mutex::new(CertMtimes::default()),
            cancel: CancellationToken::new(),
        };

        mgr.update_mtimes()?;

        if mgr.config.hot_reload {
            mgr.spawn_reload_watcher();
        }

        Ok(mgr)
    }

    fn load_configs(
        config: &TlsConfig,
    ) -> Result<(rustls::ServerConfig, rustls::ClientConfig), DerivaError> {
        let ca_certs = Self::load_pem_certs(&config.ca_cert_path)?;
        let node_certs = Self::load_pem_certs(&config.node_cert_path)?;
        let node_key = Self::load_private_key(&config.node_key_path)?;

        // Build root cert store with CA
        let mut root_store = rustls::RootCertStore::empty();
        for cert in &ca_certs {
            root_store.add(cert.clone())
                .map_err(|e| DerivaError::Config(format!("bad CA cert: {}", e)))?;
        }

        // Server config: present node cert, require client cert (mTLS)
        let client_verifier = if config.inter_node_mtls {
            rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store.clone()))
                .build()
                .map_err(|e| DerivaError::Config(format!("client verifier: {}", e)))?
        } else {
            rustls::server::WebPkiClientVerifier::no_client_auth()
        };

        let server_config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(node_certs.clone(), node_key.clone_key())
            .map_err(|e| DerivaError::Config(format!("server config: {}", e)))?;

        // Client config: present node cert, verify server against CA
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(node_certs, node_key)
            .map_err(|e| DerivaError::Config(format!("client config: {}", e)))?;

        Ok((server_config, client_config))
    }

    fn load_pem_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, DerivaError> {
        let file = std::fs::File::open(path)
            .map_err(|e| DerivaError::Config(format!("open {}: {}", path.display(), e)))?;
        let mut reader = std::io::BufReader::new(file);
        rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DerivaError::Config(format!("parse {}: {}", path.display(), e)))
    }

    fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, DerivaError> {
        let file = std::fs::File::open(path)
            .map_err(|e| DerivaError::Config(format!("open {}: {}", path.display(), e)))?;
        let mut reader = std::io::BufReader::new(file);
        rustls_pemfile::private_key(&mut reader)
            .map_err(|e| DerivaError::Config(format!("parse {}: {}", path.display(), e)))?
            .ok_or_else(|| DerivaError::Config(format!("no key in {}", path.display())))
    }

    /// Get tonic ServerTlsConfig for accepting connections.
    pub fn server_tls(&self) -> tonic::transport::ServerTlsConfig {
        let config = self.server_config.load();
        tonic::transport::ServerTlsConfig::new()
            .rustls_server_config((**config).clone())
    }

    /// Get tonic ClientTlsConfig for outgoing connections.
    pub fn client_tls(&self) -> tonic::transport::ClientTlsConfig {
        let config = self.client_config.load();
        tonic::transport::ClientTlsConfig::new()
            .rustls_client_config((**config).clone())
    }
}
```

### 3.2 Hot-Reload Watcher

```rust
impl TlsManager {
    fn spawn_reload_watcher(&self) {
        let server_config = self.server_config.clone();
        let client_config = self.client_config.clone();
        let config = self.config.clone();
        let cert_mtimes = self.cert_mtimes.lock().unwrap().clone();
        let interval = config.reload_interval;
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let mut mtimes = cert_mtimes;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = ticker.tick() => {}
                }

                let changed = Self::check_file_changed(&config.ca_cert_path, &mut mtimes.ca)
                    || Self::check_file_changed(&config.node_cert_path, &mut mtimes.cert)
                    || Self::check_file_changed(&config.node_key_path, &mut mtimes.key);

                if !changed {
                    continue;
                }

                tracing::info!("TLS certificate change detected, reloading");

                match Self::load_configs(&config) {
                    Ok((new_server, new_client)) => {
                        server_config.store(Arc::new(new_server));
                        client_config.store(Arc::new(new_client));
                        tracing::info!("TLS certificates reloaded successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "TLS reload failed, keeping old certs");
                    }
                }
            }
        });
    }

    fn check_file_changed(path: &Path, last_mtime: &mut Option<SystemTime>) -> bool {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok();
        if mtime != *last_mtime {
            *last_mtime = mtime;
            true
        } else {
            false
        }
    }

    fn update_mtimes(&self) -> Result<(), DerivaError> {
        let mut mtimes = self.cert_mtimes.lock().unwrap();
        mtimes.ca = std::fs::metadata(&self.config.ca_cert_path)
            .and_then(|m| m.modified()).ok();
        mtimes.cert = std::fs::metadata(&self.config.node_cert_path)
            .and_then(|m| m.modified()).ok();
        mtimes.key = std::fs::metadata(&self.config.node_key_path)
            .and_then(|m| m.modified()).ok();
        Ok(())
    }
}
```

### 3.3 Server Integration

```rust
/// Build tonic server with TLS.
pub fn build_server(
    state: Arc<ServerState>,
    tls_manager: &TlsManager,
    config: &TlsConfig,
) -> Result<tonic::transport::Server, DerivaError> {
    let mut server = tonic::transport::Server::builder();

    if config.inter_node_mtls {
        server = server.tls_config(tls_manager.server_tls())
            .map_err(|e| DerivaError::Config(format!("server TLS: {}", e)))?;
        tracing::info!("inter-node mTLS enabled");
    } else {
        tracing::warn!("inter-node mTLS DISABLED — cluster traffic is unencrypted");
    }

    Ok(server)
}

/// Build tonic channel to peer with TLS.
pub fn connect_peer(
    addr: &str,
    tls_manager: &TlsManager,
    config: &TlsConfig,
) -> Result<tonic::transport::Channel, DerivaError> {
    let mut endpoint = tonic::transport::Channel::from_shared(format!("https://{}", addr))
        .map_err(|e| DerivaError::Config(format!("endpoint: {}", e)))?;

    if config.inter_node_mtls {
        endpoint = endpoint.tls_config(tls_manager.client_tls())
            .map_err(|e| DerivaError::Config(format!("client TLS: {}", e)))?;
    }

    let channel = endpoint.connect_lazy();
    Ok(channel)
}
```

### 3.4 Client-Facing TLS (Optional)

```rust
/// Build client-facing server with optional TLS.
pub fn build_client_server(
    state: Arc<ServerState>,
    tls_manager: &TlsManager,
    config: &TlsConfig,
) -> Result<tonic::transport::Server, DerivaError> {
    let mut server = tonic::transport::Server::builder();

    if config.client_tls {
        // Use separate client certs if provided, else reuse node certs
        let tls_cfg = if config.client_cert_path.is_some() {
            let cert = std::fs::read_to_string(
                config.client_cert_path.as_ref().unwrap()
            )?;
            let key = std::fs::read_to_string(
                config.client_key_path.as_ref().unwrap()
            )?;
            let mut tls = tonic::transport::ServerTlsConfig::new()
                .identity(tonic::transport::Identity::from_pem(cert, key));

            if config.client_mtls {
                let ca = std::fs::read_to_string(&config.ca_cert_path)?;
                tls = tls.client_ca_root(tonic::transport::Certificate::from_pem(ca));
            }
            tls
        } else {
            tls_manager.server_tls()
        };

        server = server.tls_config(tls_cfg)
            .map_err(|e| DerivaError::Config(format!("client TLS: {}", e)))?;
        tracing::info!(mtls = config.client_mtls, "client TLS enabled");
    }

    Ok(server)
}
```

### 3.5 Startup Validation

```rust
/// Validate TLS configuration at startup. Fail fast on errors.
pub fn validate_tls_config(config: &TlsConfig) -> Result<(), DerivaError> {
    if !config.inter_node_mtls && !config.client_tls {
        tracing::warn!("all TLS disabled — running insecure");
        return Ok(());
    }

    // Check files exist and are readable
    if config.inter_node_mtls {
        check_readable(&config.ca_cert_path, "CA certificate")?;
        check_readable(&config.node_cert_path, "node certificate")?;
        check_readable(&config.node_key_path, "node private key")?;

        // Validate cert chain
        let ca = TlsManager::load_pem_certs(&config.ca_cert_path)?;
        let node = TlsManager::load_pem_certs(&config.node_cert_path)?;
        let key = TlsManager::load_private_key(&config.node_key_path)?;

        if ca.is_empty() {
            return Err(DerivaError::Config("CA cert file is empty".into()));
        }
        if node.is_empty() {
            return Err(DerivaError::Config("node cert file is empty".into()));
        }

        // Check cert expiry
        check_cert_expiry(&node[0], "node certificate")?;
        check_cert_expiry(&ca[0], "CA certificate")?;

        tracing::info!("TLS configuration validated successfully");
    }

    Ok(())
}

fn check_readable(path: &Path, label: &str) -> Result<(), DerivaError> {
    if !path.exists() {
        return Err(DerivaError::Config(format!(
            "{} not found: {}", label, path.display()
        )));
    }
    std::fs::File::open(path)
        .map_err(|e| DerivaError::Config(format!(
            "{} not readable: {}: {}", label, path.display(), e
        )))?;
    Ok(())
}

fn check_cert_expiry(cert: &CertificateDer, label: &str) -> Result<(), DerivaError> {
    // Parse X.509 to check notAfter
    let parsed = x509_parser::parse_x509_certificate(cert.as_ref())
        .map_err(|e| DerivaError::Config(format!("parse {}: {}", label, e)))?
        .1;

    let not_after = parsed.validity().not_after.to_datetime();
    let now = chrono::Utc::now();

    if not_after < now {
        return Err(DerivaError::Config(format!(
            "{} expired at {}", label, not_after
        )));
    }

    let days_remaining = (not_after - now).num_days();
    if days_remaining < 30 {
        tracing::warn!(
            cert = label,
            days_remaining = days_remaining,
            expires = %not_after,
            "certificate expiring soon"
        );
    }

    Ok(())
}
```

### 3.6 SWIM Gossip over TLS

```rust
/// SWIM uses UDP for failure detection and TCP for state sync.
/// TLS applies to the TCP state sync channel only.
/// UDP gossip pings use HMAC authentication (shared secret) instead of TLS.

impl Swim {
    pub fn configure_tls(&mut self, tls_manager: Arc<TlsManager>, config: &TlsConfig) {
        if config.inter_node_mtls {
            self.tcp_tls = Some(tls_manager.clone());
            tracing::info!("SWIM TCP state sync: mTLS enabled");
        }
        // UDP pings: use HMAC with cluster secret
        // (TLS not applicable to UDP)
    }
}

/// HMAC authentication for UDP gossip pings.
pub struct GossipAuthenticator {
    key: hmac::Key,
}

impl GossipAuthenticator {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, secret),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let tag = hmac::sign(&self.key, message);
        tag.as_ref().to_vec()
    }

    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        hmac::verify(&self.key, message, signature).is_ok()
    }
}
```


---

## 4. Data Flow Diagrams

### 4.1 mTLS Handshake Between Nodes

```
  Node A (client)                          Node B (server)
    │                                        │
    │──── ClientHello (TLS 1.3) ───────────►│
    │                                        │
    │◄─── ServerHello + Certificate ────────│
    │     (node-b.crt signed by CA)          │
    │                                        │
    │ Verify node-b.crt against ca.crt ✓    │
    │                                        │
    │──── Certificate (node-a.crt) ────────►│
    │──── CertificateVerify ───────────────►│
    │──── Finished ────────────────────────►│
    │                                        │
    │                    Verify node-a.crt   │
    │                    against ca.crt ✓    │
    │                                        │
    │◄─── Finished ─────────────────────────│
    │                                        │
    │◄═══ Encrypted gRPC channel ══════════►│
    │     (FetchValue, Replicate, etc.)      │
```

### 4.2 Rogue Node Rejection

```
  Rogue Node X                             Node B (server)
    │                                        │
    │──── ClientHello ─────────────────────►│
    │                                        │
    │◄─── ServerHello + Certificate ────────│
    │                                        │
    │──── Certificate (rogue.crt) ─────────►│
    │     (signed by different CA)           │
    │                                        │
    │                    Verify rogue.crt    │
    │                    against ca.crt ✗    │
    │                    UNKNOWN CA!         │
    │                                        │
    │◄─── TLS Alert: bad_certificate ───────│
    │                                        │
    │ Connection REFUSED                     │
    │                                        │
    │ (Rogue cannot join cluster,            │
    │  cannot read data, cannot              │
    │  issue GC commands)                    │
```

### 4.3 Certificate Hot-Reload

```
  Time ──────────────────────────────────────────────────►

  t=0:  Node starts with cert v1 (expires in 30 days)
        TlsManager loads cert v1 into ArcSwap

  t=25d: Operator places new cert v2 on disk
         tls/node.crt → updated
         tls/node.key → updated

  t=25d+30s: Reload watcher detects mtime change
             load_configs() → success
             ArcSwap::store(new_config)
             Log: "TLS certificates reloaded successfully"

  t=25d+31s: New connections use cert v2
             Existing connections continue with cert v1
             (TLS session established, cert already verified)

  t=30d: cert v1 expires
         No impact — all new connections use cert v2
         Old connections eventually close and reconnect with v2
```

### 4.4 Startup Validation Failure

```
  Node A starts with expired certificate:

  main() → validate_tls_config()
    │
    ├── check ca.crt exists ✓
    ├── check node.crt exists ✓
    ├── check node.key exists ✓
    ├── parse ca.crt ✓
    ├── parse node.crt ✓
    ├── check node.crt expiry:
    │   notAfter = 2025-01-15 < now = 2026-02-15
    │   → ERROR: "node certificate expired at 2025-01-15"
    │
    └── Node REFUSES TO START
        Exit code 1
        Log: "FATAL: TLS validation failed: node certificate expired"

  Operator must provide valid cert before node can join cluster.
```

### 4.5 SWIM UDP with HMAC Authentication

```
  Node A                                   Node B
    │                                        │
    │ Ping payload: {seq=42, from=A}         │
    │ HMAC = SHA256(payload, cluster_secret)  │
    │                                        │
    │──── [payload | HMAC] ────────────────►│
    │                                        │
    │                    Verify HMAC with     │
    │                    cluster_secret ✓     │
    │                    Process ping         │
    │                                        │
    │◄─── [ack_payload | HMAC] ────────────│
    │                                        │
    │ Verify HMAC ✓                          │
    │ Ping-ack received                      │

  Rogue node without cluster_secret:
    │──── [payload | bad_HMAC] ────────────►│
    │                    Verify HMAC ✗       │
    │                    DROP packet          │
    │                    Log: "invalid HMAC   │
    │                    from <ip>"           │
```

---

## 5. Test Specification

### 5.1 Certificate Loading Tests

```rust
#[cfg(test)]
mod cert_tests {
    use super::*;
    use tempfile::TempDir;

    fn generate_test_certs(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
        // Use rcgen to generate test CA + node cert
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(vec![]);
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let node_key = rcgen::KeyPair::generate().unwrap();
        let mut node_params = rcgen::CertificateParams::new(vec!["localhost".into()]);
        node_params.subject_alt_names = vec![
            rcgen::SanType::DnsName("localhost".try_into().unwrap()),
            rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::LOCALHOST,
            )),
        ];
        let node_cert = node_params.signed_by(&node_key, &ca_cert, &ca_key).unwrap();

        let ca_path = dir.join("ca.crt");
        let cert_path = dir.join("node.crt");
        let key_path = dir.join("node.key");

        std::fs::write(&ca_path, ca_cert.pem()).unwrap();
        std::fs::write(&cert_path, node_cert.pem()).unwrap();
        std::fs::write(&key_path, node_key.serialize_pem()).unwrap();

        (ca_path, cert_path, key_path)
    }

    #[test]
    fn test_load_valid_certs() {
        let dir = TempDir::new().unwrap();
        let (ca, cert, key) = generate_test_certs(dir.path());

        let config = TlsConfig {
            ca_cert_path: ca,
            node_cert_path: cert,
            node_key_path: key,
            ..Default::default()
        };

        let mgr = TlsManager::new(config);
        assert!(mgr.is_ok());
    }

    #[test]
    fn test_missing_ca_cert_fails() {
        let dir = TempDir::new().unwrap();
        let config = TlsConfig {
            ca_cert_path: dir.path().join("nonexistent.crt"),
            node_cert_path: dir.path().join("node.crt"),
            node_key_path: dir.path().join("node.key"),
            ..Default::default()
        };

        let result = TlsManager::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_key_fails() {
        let dir = TempDir::new().unwrap();
        let (ca, cert, _key) = generate_test_certs(dir.path());

        // Generate a different key
        let wrong_key = rcgen::KeyPair::generate().unwrap();
        let wrong_key_path = dir.path().join("wrong.key");
        std::fs::write(&wrong_key_path, wrong_key.serialize_pem()).unwrap();

        let config = TlsConfig {
            ca_cert_path: ca,
            node_cert_path: cert,
            node_key_path: wrong_key_path,
            ..Default::default()
        };

        let result = TlsManager::new(config);
        assert!(result.is_err());
    }
}
```

### 5.2 Validation Tests

```rust
#[test]
fn test_validate_expired_cert() {
    let dir = TempDir::new().unwrap();
    // Generate cert with notAfter in the past
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca_params = rcgen::CertificateParams::new(vec![]);
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.not_after = rcgen::date_time_ymd(2020, 1, 1);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();

    let ca_path = dir.path().join("ca.crt");
    std::fs::write(&ca_path, ca_cert.pem()).unwrap();

    let cert_der = CertificateDer::from(ca_cert.der().to_vec());
    let result = check_cert_expiry(&cert_der, "test CA");
    assert!(result.is_err());
}

#[test]
fn test_validate_soon_expiring_cert_warns() {
    // Generate cert expiring in 15 days
    // Should succeed but log a warning
    let dir = TempDir::new().unwrap();
    let (ca, cert, key) = generate_certs_with_expiry(dir.path(), 15);

    let config = TlsConfig {
        ca_cert_path: ca,
        node_cert_path: cert,
        node_key_path: key,
        ..Default::default()
    };

    // Should succeed (not expired yet)
    let result = validate_tls_config(&config);
    assert!(result.is_ok());
    // Warning logged (verified via tracing test subscriber)
}
```

### 5.3 Hot-Reload Tests

```rust
#[tokio::test]
async fn test_hot_reload_detects_change() {
    let dir = TempDir::new().unwrap();
    let (ca, cert, key) = generate_test_certs(dir.path());

    let config = TlsConfig {
        ca_cert_path: ca.clone(),
        node_cert_path: cert.clone(),
        node_key_path: key.clone(),
        hot_reload: true,
        reload_interval: Duration::from_millis(100), // fast for test
        ..Default::default()
    };

    let mgr = TlsManager::new(config).unwrap();
    let initial_config = mgr.server_config.load();

    // Regenerate certs (new key pair)
    let (_, new_cert, new_key) = generate_test_certs(dir.path());
    std::fs::copy(&new_cert, &cert).unwrap();
    std::fs::copy(&new_key, &key).unwrap();

    // Wait for reload
    tokio::time::sleep(Duration::from_millis(300)).await;

    let reloaded_config = mgr.server_config.load();
    // Config should be different (new cert loaded)
    assert!(!Arc::ptr_eq(&initial_config, &reloaded_config));
}

#[tokio::test]
async fn test_hot_reload_bad_cert_keeps_old() {
    let dir = TempDir::new().unwrap();
    let (ca, cert, key) = generate_test_certs(dir.path());

    let config = TlsConfig {
        ca_cert_path: ca,
        node_cert_path: cert.clone(),
        node_key_path: key,
        hot_reload: true,
        reload_interval: Duration::from_millis(100),
        ..Default::default()
    };

    let mgr = TlsManager::new(config).unwrap();
    let initial_config = mgr.server_config.load();

    // Write garbage to cert file
    std::fs::write(&cert, "not a valid cert").unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Should keep old config
    let current = mgr.server_config.load();
    assert!(Arc::ptr_eq(&initial_config, &current));
}
```

### 5.4 HMAC Authentication Tests

```rust
#[test]
fn test_hmac_sign_verify() {
    let auth = GossipAuthenticator::new(b"cluster_secret_123");
    let message = b"ping:seq=42:from=nodeA";
    let sig = auth.sign(message);
    assert!(auth.verify(message, &sig));
}

#[test]
fn test_hmac_wrong_secret_fails() {
    let auth1 = GossipAuthenticator::new(b"secret_1");
    let auth2 = GossipAuthenticator::new(b"secret_2");
    let message = b"ping:seq=42";
    let sig = auth1.sign(message);
    assert!(!auth2.verify(message, &sig));
}

#[test]
fn test_hmac_tampered_message_fails() {
    let auth = GossipAuthenticator::new(b"secret");
    let sig = auth.sign(b"original");
    assert!(!auth.verify(b"tampered", &sig));
}
```

### 5.5 Integration Tests

```rust
#[tokio::test]
async fn test_mtls_connection_between_nodes() {
    let dir = TempDir::new().unwrap();
    let (ca, cert_a, key_a) = generate_test_certs(dir.path());
    let (_, cert_b, key_b) = generate_node_cert(dir.path(), &ca, "node_b");

    let server = start_tls_server(&ca, &cert_b, &key_b, "127.0.0.1:0").await;
    let addr = server.local_addr();

    let client = connect_tls_client(&ca, &cert_a, &key_a, &addr).await;
    let result = client.health_check().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wrong_ca_rejected() {
    let dir = TempDir::new().unwrap();
    let (ca_1, cert_1, key_1) = generate_test_certs(dir.path());

    let dir2 = TempDir::new().unwrap();
    let (ca_2, cert_2, key_2) = generate_test_certs(dir2.path());

    // Server uses CA 1
    let server = start_tls_server(&ca_1, &cert_1, &key_1, "127.0.0.1:0").await;
    let addr = server.local_addr();

    // Client uses CA 2 — should fail
    let result = connect_tls_client(&ca_2, &cert_2, &key_2, &addr).await;
    assert!(result.is_err() || result.unwrap().health_check().await.is_err());
}

#[tokio::test]
async fn test_plaintext_client_rejected_by_mtls_server() {
    let dir = TempDir::new().unwrap();
    let (ca, cert, key) = generate_test_certs(dir.path());

    let server = start_tls_server(&ca, &cert, &key, "127.0.0.1:0").await;
    let addr = server.local_addr();

    // Connect without TLS
    let result = connect_plaintext_client(&addr).await;
    assert!(result.is_err());
}
```


---

## 6. Edge Cases & Error Handling

| # | Case | Behavior | Rationale |
|---|------|----------|-----------|
| 1 | Cert file missing at startup | Node refuses to start with clear error | Fail fast, no insecure fallback |
| 2 | Cert expires while node running | Existing connections unaffected; new connections fail until reload | TLS session already established |
| 3 | Hot-reload with invalid cert | Keep old cert, log error | Never downgrade to broken config |
| 4 | CA cert rotated | All nodes must get new CA + new node certs | Coordinated rotation required |
| 5 | Key file permissions too open | Log warning (not enforced) | Operator responsibility |
| 6 | Self-signed cert (no CA) | Rejected — CA is required for mTLS | Prevents accidental insecure setup |
| 7 | Cert with wrong SAN | TLS handshake fails on client side | Hostname verification |
| 8 | Clock skew > cert validity | Cert appears expired/not-yet-valid | NTP required for cluster |
| 9 | HMAC secret empty | Node refuses to start | No unauthenticated gossip |
| 10 | Mixed TLS/non-TLS cluster | TLS nodes reject non-TLS peers | Prevents partial security |

### 6.1 Certificate Rotation Procedure

```
Zero-downtime cert rotation (recommended):

  1. Generate new CA (ca-v2.crt) and new node certs signed by ca-v2
  2. Create bundle: cat ca-v1.crt ca-v2.crt > ca-bundle.crt
  3. Deploy ca-bundle.crt to all nodes (hot-reload picks it up)
     Now: nodes trust both old and new CA
  4. Deploy new node certs (node-v2.crt, node-v2.key) to all nodes
     Hot-reload picks up new certs
     Now: all nodes present new certs, trust both CAs
  5. After all nodes reloaded: remove ca-v1.crt from bundle
     Deploy ca-v2.crt only
     Now: only new CA trusted

  Total time: ~2 minutes with 30s reload interval.
  Zero connection drops (existing sessions unaffected).
```

### 6.2 Debugging TLS Failures

```
Common errors and diagnostics:

  "certificate verify failed":
    → Node cert not signed by expected CA
    → Check: openssl verify -CAfile ca.crt node.crt

  "certificate has expired":
    → Check: openssl x509 -in node.crt -noout -dates
    → Fix: issue new cert

  "no suitable certificate":
    → Key doesn't match cert
    → Check: openssl x509 -in node.crt -noout -modulus | md5sum
             openssl rsa -in node.key -noout -modulus | md5sum
    → Both should match

  "handshake failure":
    → TLS version mismatch (node requires 1.3, peer only supports 1.2)
    → Check min_tls_version config
```

---

## 7. Performance Analysis

### 7.1 TLS Handshake Overhead

```
┌──────────────────────────────┬──────────┬──────────────────────┐
│ Operation                    │ Latency  │ Notes                │
├──────────────────────────────┼──────────┼──────────────────────┤
│ TLS 1.3 handshake (RSA 2048)│ ~1.5ms   │ 1-RTT               │
│ TLS 1.3 handshake (ECDSA)   │ ~0.8ms   │ 1-RTT, faster       │
│ TLS 1.3 resumption (0-RTT)  │ ~0.3ms   │ Cached session       │
│ HMAC-SHA256 sign (UDP)       │ ~1μs     │ Per gossip message   │
│ HMAC-SHA256 verify (UDP)     │ ~1μs     │ Per gossip message   │
├──────────────────────────────┼──────────┼──────────────────────┤
│ Encryption overhead per msg  │ ~5μs     │ AES-256-GCM          │
│ Throughput impact            │ <3%      │ With AES-NI          │
└──────────────────────────────┴──────────┴──────────────────────┘

  With connection pooling (§3.13): handshake happens once per peer.
  Amortized over thousands of RPCs → negligible.
```

### 7.2 Hot-Reload Cost

```
Reload check: stat() 3 files every 30s → ~3μs every 30s → negligible.
Reload execution: parse PEM + build rustls config → ~5ms (rare event).
ArcSwap store: ~10ns (lock-free atomic pointer swap).
```

### 7.3 Memory Overhead

```
Per TLS session: ~20KB (rustls session state)
Per peer connection: 1 session × 20KB = 20KB
16-node cluster: 15 peers × 20KB = 300KB
Cert storage: ~10KB (CA + node cert + key in memory)

Total: <1MB for TLS state. Negligible.
```

### 7.4 Benchmarking Plan

```rust
/// Benchmark: TLS handshake latency
#[bench]
fn bench_tls_handshake(b: &mut Bencher) {
    // Full mTLS handshake over loopback
    // Expected: <2ms
}

/// Benchmark: encrypted RPC vs plaintext
#[bench]
fn bench_rpc_with_tls(b: &mut Bencher) {
    // GetValue RPC with mTLS vs without
    // Expected: <5% throughput difference
}

/// Benchmark: HMAC sign + verify
#[bench]
fn bench_hmac_roundtrip(b: &mut Bencher) {
    // Sign + verify 100-byte gossip message
    // Expected: <3μs
}

/// Benchmark: cert reload
#[bench]
fn bench_cert_reload(b: &mut Bencher) {
    // Full load_configs() cycle
    // Expected: <10ms
}
```

---

## 8. Files Changed

| File | Change |
|------|--------|
| `deriva-network/src/tls.rs` | **NEW** — TlsManager, TlsConfig, hot-reload |
| `deriva-network/src/gossip_auth.rs` | **NEW** — GossipAuthenticator (HMAC) |
| `deriva-network/src/lib.rs` | Add `pub mod tls`, `pub mod gossip_auth` |
| `deriva-network/src/swim.rs` | Add `configure_tls()`, HMAC on UDP pings |
| `deriva-server/src/main.rs` | Initialize TlsManager, pass to server builder |
| `deriva-server/src/state.rs` | Add `tls_manager: Option<Arc<TlsManager>>` |
| `deriva-network/src/peer_channel.rs` | Use `client_tls()` when connecting to peers |
| `deriva-network/tests/tls.rs` | **NEW** — unit + integration tests |

---

## 9. Dependency Changes

| Crate | Version | Purpose |
|-------|---------|---------|
| `rustls` | 0.23.x | TLS implementation (already via tonic) |
| `rustls-pemfile` | 2.x | PEM parsing |
| `arc-swap` | 1.x | Lock-free config hot-swap |
| `ring` | 0.17.x | HMAC-SHA256 for gossip auth |
| `x509-parser` | 0.16.x | Certificate expiry validation |
| `rcgen` | 0.13.x | **dev-dependency** — test cert generation |

---

## 10. Design Rationale

### 10.1 Why mTLS Instead of Token-Based Auth?

```
Token-based (e.g., shared secret in gRPC metadata):
  + Simpler setup (one secret, no PKI)
  - No encryption (need separate TLS layer anyway)
  - Token rotation requires coordinated restart
  - No per-node identity (all nodes share same token)

mTLS:
  + Encryption + authentication in one mechanism
  + Per-node identity (each node has unique cert)
  + Standard PKI tooling (openssl, cert-manager)
  + Hot-reload without restart
  - More complex initial setup (CA, certs)

  mTLS is industry standard for service mesh / inter-node auth.
  The setup complexity is a one-time cost.
```

### 10.2 Why HMAC for UDP Gossip Instead of DTLS?

```
DTLS (TLS over UDP):
  + Full encryption + authentication
  - Complex: handshake state machine over unreliable transport
  - High overhead for small gossip pings (64 bytes)
  - Few mature Rust DTLS libraries

HMAC with shared secret:
  + Simple: append 32-byte tag to each message
  + Low overhead: ~1μs per message
  + Prevents impersonation and tampering
  - No encryption (gossip content visible)
  - Shared secret (not per-node)

  Gossip pings contain only membership state (node IDs, incarnations).
  This is not sensitive data. Authentication (preventing rogue nodes
  from injecting false membership info) is the primary concern.
  HMAC is sufficient and much simpler than DTLS.
```

### 10.3 Why ArcSwap Instead of RwLock for Config?

```
RwLock:
  - Read lock on every TLS operation (every RPC)
  - Write lock during reload blocks all RPCs briefly
  - Potential priority inversion

ArcSwap:
  - Lock-free reads (~10ns, same as Arc clone)
  - Store is atomic pointer swap
  - No blocking, no priority inversion
  - Readers never block writers, writers never block readers

  For a config that's read millions of times per second
  and written once per 30 seconds, ArcSwap is ideal.
```

### 10.4 Why Fail-Closed on TLS Misconfiguration?

```
Fail-open: if TLS config is bad, run without TLS.
  Dangerous: operator thinks cluster is secure, but it's not.
  Silent security degradation.

Fail-closed: if TLS config is bad, refuse to start.
  Operator immediately knows something is wrong.
  No silent insecurity.

  Exception: hot-reload failure is fail-open (keep old certs).
  Because: old certs are still valid, better than no certs.
  Only startup validation is fail-closed.
```

---

## 11. Observability Integration

### 11.1 Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `tls_handshakes_total` | Counter | `result={success,failed}` | Handshake outcomes |
| `tls_handshake_duration_ms` | Histogram | — | Handshake latency |
| `tls_cert_expiry_seconds` | Gauge | `cert={node,ca}` | Seconds until expiry |
| `tls_reloads_total` | Counter | `result={success,failed}` | Hot-reload outcomes |
| `tls_active_sessions` | Gauge | — | Current TLS sessions |
| `gossip_auth_failures` | Counter | — | HMAC verification failures |

### 11.2 Structured Logging

```rust
tracing::info!(
    ca = %config.ca_cert_path.display(),
    cert = %config.node_cert_path.display(),
    min_tls = ?config.min_tls_version,
    "TLS initialized"
);

tracing::info!("TLS certificates reloaded successfully");

tracing::error!(
    error = %e,
    "TLS reload failed, keeping old certificates"
);

tracing::warn!(
    cert = label,
    days_remaining = days_remaining,
    "certificate expiring soon — rotate before expiry"
);

tracing::warn!(
    remote_addr = %addr,
    "gossip HMAC verification failed — possible rogue node"
);
```

### 11.3 Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Cert expiring soon | `tls_cert_expiry_seconds{cert=node}` < 7 days | Warning |
| Cert expiring critical | `tls_cert_expiry_seconds{cert=node}` < 1 day | Critical |
| TLS handshake failures | `tls_handshakes_total{result=failed}` > 10/min | Warning |
| HMAC auth failures | `gossip_auth_failures` > 5/min | Warning |
| Reload failures | `tls_reloads_total{result=failed}` > 0 | Warning |

---

## 12. Checklist

- [ ] Create `deriva-network/src/tls.rs`
- [ ] Implement `TlsManager` with cert loading
- [ ] Implement `load_configs` (server + client rustls configs)
- [ ] Implement hot-reload watcher with mtime detection
- [ ] Implement `ArcSwap`-based config swap
- [ ] Implement `validate_tls_config` startup validation
- [ ] Implement cert expiry checking with warnings
- [ ] Create `deriva-network/src/gossip_auth.rs`
- [ ] Implement `GossipAuthenticator` (HMAC-SHA256)
- [ ] Integrate HMAC into SWIM UDP ping/ack
- [ ] Wire TLS into tonic server builder
- [ ] Wire TLS into peer channel connections
- [ ] Add optional client-facing TLS
- [ ] Write cert loading tests (3 tests)
- [ ] Write validation tests (2 tests)
- [ ] Write hot-reload tests (2 tests)
- [ ] Write HMAC tests (3 tests)
- [ ] Write integration tests (3 tests)
- [ ] Add metrics (6 metrics)
- [ ] Add structured log events
- [ ] Configure alerts (5 alerts)
- [ ] Run benchmarks: handshake, encrypted RPC, HMAC, reload
- [ ] Document cert rotation procedure in ops guide
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Commit and push
