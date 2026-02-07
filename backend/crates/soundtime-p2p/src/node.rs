//! Core P2P node — wraps an iroh Endpoint and iroh-blobs Store.
//!
//! The `P2pNode` is the central unit that:
//! - Listens for incoming peer connections (QUIC via iroh)
//! - Publishes local tracks into content-addressed blob storage
//! - Fetches tracks from remote peers by hash
//! - Exposes the local NodeId for discovery

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use iroh::endpoint::Connection;
use iroh::{Endpoint, NodeAddr, NodeId, SecretKey};
use iroh_blobs::store::fs::Store as FsStore;
use iroh_blobs::store::{Map, MapEntry, Store as StoreOps};
use iroh_blobs::{BlobFormat, Hash, TempTag};
use sea_orm::DatabaseConnection;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::blocked::is_peer_blocked;
use crate::discovery::PeerRegistry;
use crate::error::P2pError;

/// ALPN protocol identifier for SoundTime P2P
pub const SOUNDTIME_ALPN: &[u8] = b"soundtime/p2p/1";

/// Protocol message types exchanged between peers.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum P2pMessage {
    /// Request a track blob by its content hash
    FetchTrack { hash: String },
    /// Announce that this node has a track available
    AnnounceTrack { hash: String, title: String },
    /// Response containing track data
    TrackData { hash: String, size: u64 },
    /// Peer discovery ping
    Ping,
    /// Peer discovery pong
    Pong { node_id: String, track_count: u64 },
    /// Peer exchange — share list of known peer NodeIds for network discovery
    PeerExchange { peers: Vec<String> },
}

/// Configuration for the P2P node.
#[derive(Clone, Debug)]
pub struct P2pConfig {
    /// Directory for iroh-blobs persistent storage
    pub blobs_dir: PathBuf,
    /// Path to a persistent secret key file (ensures stable NodeId across restarts)
    pub secret_key_path: PathBuf,
    /// Bind port (0 = random)
    pub bind_port: u16,
    /// Whether to enable local network (mDNS) discovery
    pub enable_local_discovery: bool,
    /// Seed peer NodeIds to connect to on startup (auto-discovery)
    pub seed_peers: Vec<String>,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            blobs_dir: PathBuf::from("data/p2p/blobs"),
            secret_key_path: PathBuf::from("data/p2p/secret_key"),
            bind_port: 0,
            enable_local_discovery: true,
            seed_peers: Vec::new(),
        }
    }
}

impl P2pConfig {
    /// Build config from environment variables.
    pub fn from_env() -> Self {
        let blobs_dir = std::env::var("P2P_BLOBS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/p2p/blobs"));

        let secret_key_path = std::env::var("P2P_SECRET_KEY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| blobs_dir.parent()
                .unwrap_or(&blobs_dir)
                .join("secret_key"));

        let bind_port = std::env::var("P2P_BIND_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let enable_local_discovery = std::env::var("P2P_LOCAL_DISCOVERY")
            .unwrap_or_else(|_| "true".to_string())
            .eq_ignore_ascii_case("true");

        let seed_peers = std::env::var("P2P_SEED_PEERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            blobs_dir,
            secret_key_path,
            bind_port,
            enable_local_discovery,
            seed_peers,
        }
    }
}

/// The main P2P node. Wraps an iroh `Endpoint` and an iroh-blobs `FsStore`.
pub struct P2pNode {
    /// iroh QUIC endpoint
    endpoint: Endpoint,
    /// Content-addressed blob store (filesystem-backed)
    blob_store: FsStore,
    /// Database handle for peer blocking checks
    db: DatabaseConnection,
    /// Registry of known peers (in-memory)
    registry: Arc<PeerRegistry>,
    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,
    /// Configuration used to create this node
    _config: P2pConfig,
}

impl P2pNode {
    /// Start a new P2P node with the given config and database.
    pub async fn start(config: P2pConfig, db: DatabaseConnection) -> Result<Arc<Self>, P2pError> {
        // Ensure blobs directory exists
        tokio::fs::create_dir_all(&config.blobs_dir)
            .await
            .map_err(P2pError::Io)?;

        // Load or generate secret key for stable NodeId across restarts
        let secret_key = Self::load_or_generate_key(&config).await?;
        let node_id = secret_key.public();
        info!(%node_id, "starting P2P node");

        // Initialize the blob store
        let blob_store = FsStore::load(&config.blobs_dir)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        // Build the iroh endpoint with discovery services and relay
        //
        // discovery_n0() registers both:
        //   - PkarrPublisher  — publishes our NodeId + relay URL to n0's DNS server
        //   - DnsDiscovery    — resolves other NodeIds via DNS queries to n0's server
        //
        // Without these, the node cannot register with relay servers and peers
        // cannot discover us — this was the root cause of "Relay Disconnected".
        let mut builder = Endpoint::builder()
            .secret_key(secret_key)
            .alpns(vec![SOUNDTIME_ALPN.to_vec()])
            .discovery_n0();

        // Optionally enable local network discovery (mDNS/swarm)
        if config.enable_local_discovery {
            info!("enabling local network discovery (mDNS/swarm)");
            builder = builder.add_discovery(|secret_key: &SecretKey| {
                let node_id = secret_key.public();
                match iroh::discovery::local_swarm_discovery::LocalSwarmDiscovery::new(node_id) {
                    Ok(discovery) => {
                        tracing::info!("local swarm discovery started");
                        Some(discovery)
                    }
                    Err(e) => {
                        tracing::warn!("failed to start local swarm discovery: {e}");
                        None
                    }
                }
            });
        }

        // Bind to the configured port (0 = random)
        if config.bind_port > 0 {
            builder = builder.bind_addr_v4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, config.bind_port));
            info!(port = config.bind_port, "binding P2P endpoint to configured port");
        }

        let endpoint = builder
            .bind()
            .await
            .map_err(|e| P2pError::Endpoint(e.to_string()))?;

        // Wait for relay connection (up to 15 seconds)
        // With discovery services registered, the endpoint will automatically
        // connect to one of n0's production relay servers and publish its address.
        info!("waiting for relay connection...");
        match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            Self::wait_for_relay(&endpoint),
        )
        .await
        {
            Ok(Some(relay)) => info!(%relay, "connected to relay server"),
            Ok(None) => warn!("no relay server available — P2P will use direct connections only"),
            Err(_) => warn!("relay connection timed out after 15s — continuing without relay"),
        }

        // Log direct addresses for diagnostics
        if let Ok(addr) = endpoint.node_addr().await {
            info!(
                direct_addrs = addr.direct_addresses.len(),
                relay = ?addr.relay_url.as_ref().map(|u| u.to_string()),
                "P2P endpoint bound"
            );
        }

        let (shutdown_tx, _) = watch::channel(false);

        let registry = Arc::new(PeerRegistry::new());

        let node = Arc::new(Self {
            endpoint,
            blob_store,
            db,
            registry,
            shutdown_tx,
            _config: config,
        });

        // Spawn the connection accept loop
        let node_clone = Arc::clone(&node);
        tokio::spawn(async move {
            node_clone.accept_loop().await;
        });

        // Connect to seed peers in background (after a short delay for relay setup)
        if !node._config.seed_peers.is_empty() {
            let seed_peers = node._config.seed_peers.clone();
            let node_clone = Arc::clone(&node);
            tokio::spawn(async move {
                // Give the relay a moment to fully stabilize
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                node_clone.connect_to_seed_peers(&seed_peers).await;
            });
        }

        // Spawn periodic peer exchange & refresh (every 5 minutes)
        {
            let node_clone = Arc::clone(&node);
            let mut shutdown_rx = node.shutdown_tx.subscribe();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                interval.tick().await; // skip first immediate tick
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let peers = node_clone.registry.online_peers().await;
                            if !peers.is_empty() {
                                info!(online = peers.len(), "periodic peer exchange");
                                if let Ok(nid) = peers[0].node_id.parse::<NodeId>() {
                                    node_clone.discover_via_peer(nid).await;
                                }
                            }
                        }
                        _ = shutdown_rx.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        info!("P2P node started successfully");
        Ok(node)
    }

    /// Get this node's unique identifier (public key).
    pub fn node_id(&self) -> NodeId {
        self.endpoint.node_id()
    }

    /// Get the peer registry.
    pub fn registry(&self) -> &Arc<PeerRegistry> {
        &self.registry
    }

    /// Get the node address (includes relay URL and direct addresses).
    pub async fn node_addr(&self) -> Result<NodeAddr, P2pError> {
        self.endpoint
            .node_addr()
            .await
            .map_err(|e| P2pError::Endpoint(e.to_string()))
    }

    /// Get the relay URL this node is connected to (if any).
    /// By default, iroh connects to n0.computer production relay servers.
    pub async fn relay_url(&self) -> Option<String> {
        self.node_addr()
            .await
            .ok()
            .and_then(|addr| addr.relay_url.map(|u| u.to_string()))
    }

    /// Get the number of direct addresses the endpoint is listening on.
    pub async fn direct_addresses_count(&self) -> usize {
        self.node_addr()
            .await
            .map(|addr| addr.direct_addresses.len())
            .unwrap_or(0)
    }

    /// Publish a track's audio data to the local blob store.
    /// Returns the content hash (BLAKE3) that identifies the blob.
    pub async fn publish_track(&self, data: Bytes) -> Result<Hash, P2pError> {
        let tt: TempTag = self
            .blob_store
            .import_bytes(data, BlobFormat::Raw)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        let hash = *tt.hash();
        info!(%hash, "track published to blob store");
        Ok(hash)
    }

    /// Retrieve a track's data from the local blob store by hash.
    pub async fn get_local_track(&self, hash: Hash) -> Result<Bytes, P2pError> {
        use iroh_io::AsyncSliceReader;

        let entry = self
            .blob_store
            .get(&hash)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?
            .ok_or_else(|| P2pError::TrackNotFound(hash.to_string()))?;

        let size = entry.size().value();
        let mut reader = entry.data_reader();

        let data: Bytes = AsyncSliceReader::read_at(&mut reader, 0, size as usize)
            .await
            .map_err(|e: std::io::Error| P2pError::BlobStore(e.to_string()))?;

        Ok(data)
    }

    /// Check if a blob exists locally.
    pub async fn has_blob(&self, hash: Hash) -> bool {
        self.blob_store
            .get(&hash)
            .await
            .ok()
            .and_then(|e| e)
            .is_some()
    }

    /// Connect to a remote peer and fetch a track by its content hash.
    pub async fn fetch_track_from_peer(
        &self,
        peer_addr: NodeAddr,
        hash: Hash,
    ) -> Result<Bytes, P2pError> {
        let peer_id = peer_addr.node_id.to_string();

        // Check if peer is blocked
        if is_peer_blocked(&self.db, &peer_id).await {
            return Err(P2pError::PeerBlocked(peer_id));
        }

        info!(peer = %peer_id, %hash, "fetching track from peer");

        let conn = self
            .endpoint
            .connect(peer_addr, SOUNDTIME_ALPN)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        // Send fetch request
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let request = P2pMessage::FetchTrack {
            hash: hash.to_string(),
        };
        let request_bytes = serde_json::to_vec(&request)?;
        send.write_all(&(request_bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.write_all(&request_bytes)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        // Read response — first 4 bytes = length, then data
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        let data_len = u32::from_be_bytes(len_buf) as usize;

        if data_len == 0 {
            return Err(P2pError::TrackNotFound(hash.to_string()));
        }

        let data = recv
            .read_to_end(data_len)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        debug!(%hash, bytes = data.len(), "received track from peer");
        Ok(Bytes::from(data))
    }

    /// Send a ping to a peer and wait for pong.
    pub async fn ping_peer(&self, peer_addr: NodeAddr) -> Result<P2pMessage, P2pError> {
        let conn = self
            .endpoint
            .connect(peer_addr, SOUNDTIME_ALPN)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let ping = serde_json::to_vec(&P2pMessage::Ping)?;
        send.write_all(&(ping.len() as u32).to_be_bytes())
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.write_all(&ping)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        let msg_len = u32::from_be_bytes(len_buf) as usize;

        let response = recv
            .read_to_end(msg_len)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let pong: P2pMessage = serde_json::from_slice(&response)?;
        Ok(pong)
    }

    /// Connect to a list of seed peers by NodeId.
    /// Pings each one and registers it in the registry.
    async fn connect_to_seed_peers(&self, seed_peers: &[String]) {
        info!(count = seed_peers.len(), "connecting to seed peers");

        for peer_id_str in seed_peers {
            let node_id: NodeId = match peer_id_str.parse() {
                Ok(id) => id,
                Err(e) => {
                    warn!(peer = %peer_id_str, "invalid seed peer NodeId: {e}");
                    continue;
                }
            };

            // Skip ourselves
            if node_id == self.node_id() {
                debug!("skipping self in seed peers");
                continue;
            }

            let peer_addr = NodeAddr::new(node_id);
            info!(peer = %peer_id_str, "pinging seed peer");

            match self.ping_peer(peer_addr).await {
                Ok(P2pMessage::Pong { node_id: nid, track_count }) => {
                    self.registry.upsert_peer(&nid, None, track_count).await;
                    info!(peer = %nid, %track_count, "seed peer connected and registered");
                    // Exchange peer lists to discover the wider network
                    self.discover_via_peer(node_id).await;
                }
                Ok(_) => {
                    self.registry.upsert_peer(peer_id_str, None, 0).await;
                    warn!(peer = %peer_id_str, "seed peer responded with unexpected message");
                }
                Err(e) => {
                    warn!(peer = %peer_id_str, "failed to reach seed peer: {e}");
                }
            }
        }

        let total = self.registry.peer_count().await;
        let online = self.registry.online_peers().await.len();
        info!(%total, %online, "seed peer discovery complete");
    }

    /// Send a message to a specific peer.
    async fn send_message_to_peer(&self, node_id: NodeId, msg: &P2pMessage) -> Result<(), P2pError> {
        let peer_addr = NodeAddr::new(node_id);
        let conn = self
            .endpoint
            .connect(peer_addr, SOUNDTIME_ALPN)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let (mut send, _recv) = conn
            .open_bi()
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let msg_bytes = serde_json::to_vec(msg)?;
        send.write_all(&(msg_bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.write_all(&msg_bytes)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        Ok(())
    }

    /// Broadcast a track announcement to all online peers.
    /// Called after a track is published to the local blob store.
    pub async fn broadcast_announce_track(&self, hash: Hash, title: String) {
        let peers = self.registry.online_peers().await;
        if peers.is_empty() {
            debug!(%hash, "no online peers to announce track to");
            return;
        }

        info!(%hash, %title, peer_count = peers.len(), "broadcasting track announcement");

        let msg = P2pMessage::AnnounceTrack {
            hash: hash.to_string(),
            title,
        };

        for peer in &peers {
            let node_id: NodeId = match peer.node_id.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };

            if let Err(e) = self.send_message_to_peer(node_id, &msg).await {
                warn!(peer = %peer.node_id, "failed to announce track: {e}");
                self.registry.mark_offline(&peer.node_id).await;
            } else {
                debug!(peer = %peer.node_id, %hash, "track announced");
            }
        }
    }

    /// Discover peers by exchanging peer lists with a known peer (Peer Exchange / PEX).
    /// Sends our known peers, receives theirs, and connects to any new ones.
    pub async fn discover_via_peer(&self, peer_node_id: NodeId) {
        info!(peer = %peer_node_id, "initiating peer exchange");

        let peer_addr = NodeAddr::new(peer_node_id);
        match self.exchange_peers(peer_addr).await {
            Ok(remote_peers) => {
                let our_id = self.node_id().to_string();
                let mut new_count = 0u32;

                for peer_id_str in remote_peers {
                    if peer_id_str == our_id {
                        continue;
                    }
                    if self.registry.get_peer(&peer_id_str).await.is_some() {
                        continue;
                    }

                    let nid: NodeId = match peer_id_str.parse() {
                        Ok(id) => id,
                        Err(_) => continue,
                    };

                    let addr = NodeAddr::new(nid);
                    match self.ping_peer(addr).await {
                        Ok(P2pMessage::Pong {
                            node_id,
                            track_count,
                        }) => {
                            self.registry.upsert_peer(&node_id, None, track_count).await;
                            info!(peer = %node_id, %track_count, "discovered new peer via PEX");
                            new_count += 1;
                        }
                        Ok(_) => {
                            self.registry.upsert_peer(&peer_id_str, None, 0).await;
                            new_count += 1;
                        }
                        Err(e) => {
                            debug!(peer = %peer_id_str, "PEX peer unreachable: {e}");
                        }
                    }
                }

                info!(peer = %peer_node_id, %new_count, "peer exchange complete");
            }
            Err(e) => {
                warn!(peer = %peer_node_id, "peer exchange failed: {e}");
            }
        }
    }

    /// Send our peer list to a remote peer and receive theirs.
    async fn exchange_peers(&self, peer_addr: NodeAddr) -> Result<Vec<String>, P2pError> {
        let conn = self
            .endpoint
            .connect(peer_addr, SOUNDTIME_ALPN)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        // Build our peer list (include ourselves so remote knows us)
        let mut our_peers: Vec<String> = self
            .registry
            .list_peers()
            .await
            .into_iter()
            .map(|p| p.node_id)
            .collect();
        our_peers.push(self.node_id().to_string());

        let msg = P2pMessage::PeerExchange { peers: our_peers };
        let msg_bytes = serde_json::to_vec(&msg)?;
        send.write_all(&(msg_bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.write_all(&msg_bytes)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        // Read response
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        let msg_len = u32::from_be_bytes(len_buf) as usize;

        let response = recv
            .read_to_end(msg_len)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let response_msg: P2pMessage = serde_json::from_slice(&response)?;

        match response_msg {
            P2pMessage::PeerExchange { peers } => Ok(peers),
            _ => Ok(vec![]),
        }
    }

    /// Gracefully shutdown the P2P node.
    pub async fn shutdown(&self) {
        info!("shutting down P2P node");
        let _ = self.shutdown_tx.send(true);
        self.endpoint.close().await;
        StoreOps::shutdown(&self.blob_store).await;
        info!("P2P node shutdown complete");
    }

    /// Internal: accept incoming connections in a loop.
    async fn accept_loop(self: &Arc<Self>) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    match incoming {
                        Some(incoming) => {
                            let node = Arc::clone(self);
                            tokio::spawn(async move {
                                match incoming.await {
                                    Ok(conn) => {
                                        if let Err(e) = node.handle_connection(conn).await {
                                            warn!("error handling connection: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        warn!("error accepting connection: {e}");
                                    }
                                }
                            });
                        }
                        None => {
                            info!("endpoint accept stream ended");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("accept loop received shutdown signal");
                    break;
                }
            }
        }
    }

    /// Internal: handle a single incoming connection.
    async fn handle_connection(&self, conn: Connection) -> Result<(), P2pError> {
        let peer_id = match conn.remote_node_id() {
            Ok(id) => id.to_string(),
            Err(_) => "unknown".to_string(),
        };

        info!(%peer_id, "incoming connection");

        // Check if peer is blocked
        if is_peer_blocked(&self.db, &peer_id).await {
            warn!(%peer_id, "rejected blocked peer");
            conn.close(1u8.into(), b"blocked");
            return Err(P2pError::PeerBlocked(peer_id));
        }

        // Register the peer in our registry (marks it online with last_seen = now)
        self.registry.upsert_peer(&peer_id, None, 0).await;

        // Accept bidirectional streams from this connection
        loop {
            match conn.accept_bi().await {
                Ok((send, mut recv)) => {
                    let node_id = self.node_id();

                    // Read length-prefixed message
                    let mut len_buf = [0u8; 4];
                    if recv.read_exact(&mut len_buf).await.is_err() {
                        break;
                    }
                    let msg_len = u32::from_be_bytes(len_buf) as usize;
                    let msg_bytes = match recv.read_to_end(msg_len).await {
                        Ok(b) => b,
                        Err(_) => break,
                    };

                    let msg: P2pMessage = match serde_json::from_slice(&msg_bytes) {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("invalid message from {peer_id}: {e}");
                            break;
                        }
                    };

                    self.handle_message(msg, send, node_id, &peer_id).await?;
                }
                Err(_) => break,
            }
        }

        Ok(())
    }

    /// Internal: handle a single protocol message.
    async fn handle_message(
        &self,
        msg: P2pMessage,
        mut send: iroh::endpoint::SendStream,
        node_id: NodeId,
        peer_id: &str,
    ) -> Result<(), P2pError> {
        match msg {
            P2pMessage::FetchTrack { hash } => {
                let hash: Hash = hash
                    .parse()
                    .map_err(|_| P2pError::TrackNotFound(hash.clone()))?;

                match self.get_local_track(hash).await {
                    Ok(data) => {
                        let len = data.len() as u32;
                        send.write_all(&len.to_be_bytes())
                            .await
                            .map_err(|e| P2pError::Connection(e.to_string()))?;
                        send.write_all(&data)
                            .await
                            .map_err(|e| P2pError::Connection(e.to_string()))?;
                    }
                    Err(_) => {
                        // Send zero-length response to indicate not found
                        send.write_all(&0u32.to_be_bytes())
                            .await
                            .map_err(|e| P2pError::Connection(e.to_string()))?;
                    }
                }
                send.finish()
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
            }
            P2pMessage::Ping => {
                let pong = P2pMessage::Pong {
                    node_id: node_id.to_string(),
                    track_count: 0, // TODO: get actual track count from DB
                };
                let pong_bytes = serde_json::to_vec(&pong)?;
                send.write_all(&(pong_bytes.len() as u32).to_be_bytes())
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                send.write_all(&pong_bytes)
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                send.finish()
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
            }
            P2pMessage::AnnounceTrack { hash, title } => {
                info!(%hash, %title, %peer_id, "received track announcement");
                // Update last_seen for the announcing peer
                self.registry.upsert_peer(peer_id, None, 0).await;

                // Auto-fetch: if we don't have this blob, fetch it from the announcing peer
                let blob_hash: Result<Hash, _> = hash.parse();
                if let Ok(h) = blob_hash {
                    if !self.has_blob(h).await {
                        let peer_node_id: Result<NodeId, _> = peer_id.parse();
                        if let Ok(nid) = peer_node_id {
                            info!(%hash, %peer_id, "auto-fetching announced track");
                            let peer_addr = NodeAddr::new(nid);
                            match self.fetch_track_from_peer(peer_addr, h).await {
                                Ok(data) => {
                                    // Import fetched data into our blob store
                                    match self.blob_store.import_bytes(data, BlobFormat::Raw).await {
                                        Ok(tag) => {
                                            info!(
                                                %hash,
                                                local_hash = %tag.hash(),
                                                "track replicated from peer"
                                            );
                                        }
                                        Err(e) => {
                                            warn!(%hash, "failed to import fetched track: {e}");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(%hash, %peer_id, "failed to auto-fetch track: {e}");
                                }
                            }
                        }
                    } else {
                        debug!(%hash, "already have this track, skipping fetch");
                    }
                }
            }
            P2pMessage::PeerExchange { peers } => {
                info!(count = peers.len(), %peer_id, "received peer exchange request");

                // Register new peers from the exchange (as known but unverified)
                let our_id = self.node_id().to_string();
                for pid in &peers {
                    if *pid != our_id && self.registry.get_peer(pid).await.is_none() {
                        self.registry.upsert_peer(pid, None, 0).await;
                        debug!(peer = %pid, "discovered peer via PEX");
                    }
                }

                // Reply with our peer list (including ourselves)
                let mut our_peers: Vec<String> = self
                    .registry
                    .list_peers()
                    .await
                    .into_iter()
                    .map(|p| p.node_id)
                    .collect();
                our_peers.push(our_id);

                let reply = P2pMessage::PeerExchange { peers: our_peers };
                let reply_bytes = serde_json::to_vec(&reply)?;
                send.write_all(&(reply_bytes.len() as u32).to_be_bytes())
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                send.write_all(&reply_bytes)
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                send.finish()
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
            }
            P2pMessage::TrackData { .. } | P2pMessage::Pong { .. } => {
                // These are responses, not requests — ignore if received as requests
                debug!("received unexpected response message");
            }
        }

        Ok(())
    }

    /// Load or generate a persistent secret key.
    /// The key is always persisted to ensure stable NodeId across restarts.
    async fn load_or_generate_key(config: &P2pConfig) -> Result<SecretKey, P2pError> {
        let key_path = &config.secret_key_path;

        if key_path.exists() {
            let key_bytes = tokio::fs::read(key_path).await.map_err(P2pError::Io)?;
            let key_str = String::from_utf8(key_bytes)
                .map_err(|e| P2pError::Endpoint(format!("invalid key file encoding: {e}")))?;
            let key: SecretKey = key_str
                .trim()
                .parse()
                .map_err(|e| P2pError::Endpoint(format!("invalid secret key: {e}")))?;
            info!(path = %key_path.display(), "loaded existing P2P secret key");
            return Ok(key);
        }

        // Generate and save a new key
        let key = SecretKey::generate(&mut rand::rngs::OsRng);
        if let Some(parent) = key_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(P2pError::Io)?;
        }
        tokio::fs::write(key_path, key.to_string().as_bytes())
            .await
            .map_err(P2pError::Io)?;
        info!(path = %key_path.display(), "generated and saved new P2P secret key");
        Ok(key)
    }

    /// Wait for the relay connection to be established.
    /// Returns the relay URL once connected, or None if unavailable.
    async fn wait_for_relay(endpoint: &Endpoint) -> Option<String> {
        // Poll node_addr until relay_url is available
        for _ in 0..50 {
            if let Ok(addr) = endpoint.node_addr().await {
                if let Some(url) = addr.relay_url {
                    return Some(url.to_string());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        None
    }
}
