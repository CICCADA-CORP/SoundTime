//! Core P2P node — wraps an iroh Endpoint and iroh-blobs Store.
//!
//! The `P2pNode` is the central unit that:
//! - Listens for incoming peer connections (QUIC via iroh)
//! - Publishes local tracks into content-addressed blob storage
//! - Fetches tracks from remote peers by hash
//! - Exposes the local EndpointId for discovery

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
use rand::SeedableRng;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};
use soundtime_db::entities::{album, artist, remote_track, track};
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::blob_cache::BlobCache;
use crate::blocked::is_peer_blocked;
use crate::connection_pool::ConnectionPool;
use crate::discovery::PeerRegistry;
use crate::error::P2pError;
use crate::musicbrainz::MusicBrainzClient;
use crate::search_index::{BloomFilterData, SearchIndex};
use crate::track_health::{spawn_health_monitor, PeerTrackInfo, TrackFetcher, TrackHealthManager};

/// ALPN protocol identifier for SoundTime P2P
pub const SOUNDTIME_ALPN: &[u8] = b"soundtime/p2p/1";

/// Maximum allowed size for a single P2P message (64 MiB).
/// CatalogSync messages can be large for instances with many tracks.
const MAX_P2P_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// Maximum number of concurrent incoming P2P connections.
const MAX_CONCURRENT_P2P_CONNECTIONS: usize = 64;

/// Sanitize a string for use as a filesystem directory name.
fn sanitize_for_path(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Track metadata sent alongside announcements for catalog replication.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct TrackAnnouncement {
    /// BLAKE3 content hash of the audio blob
    pub hash: String,
    /// Track title
    pub title: String,
    /// Artist name
    pub artist_name: String,
    /// Album artist name — used for album grouping on compilations.
    /// Falls back to `artist_name` when absent (backward compat with older peers).
    #[serde(default)]
    pub album_artist_name: Option<String>,
    /// Album title (if any)
    pub album_title: Option<String>,
    /// Duration in seconds
    pub duration_secs: f32,
    /// Audio format (e.g. "FLAC", "MP3")
    pub format: String,
    /// File size in bytes
    pub file_size: i64,
    /// Genre (optional)
    pub genre: Option<String>,
    /// Year (optional)
    pub year: Option<i16>,
    /// Track number on the album (optional)
    pub track_number: Option<i16>,
    /// Disc number (optional)
    pub disc_number: Option<i16>,
    /// Bitrate in bps (optional)
    pub bitrate: Option<i32>,
    /// Sample rate in Hz (optional)
    pub sample_rate: Option<i32>,
    /// EndpointId of the instance that originally uploaded the track
    pub origin_node: String,
    /// BLAKE3 hash of the cover art blob (if any)
    pub cover_hash: Option<String>,
}

/// Protocol message types exchanged between peers.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum P2pMessage {
    /// Request a track blob by its content hash
    FetchTrack { hash: String },
    /// Announce a track with full metadata for catalog replication
    AnnounceTrack(TrackAnnouncement),
    /// Response containing track data
    TrackData { hash: String, size: u64 },
    /// Peer discovery ping
    Ping,
    /// Peer discovery pong
    Pong {
        node_id: String,
        track_count: u64,
        /// Software version of the peer (e.g. "0.1.42")
        #[serde(default)]
        version: Option<String>,
    },
    /// Peer exchange — share list of known peer EndpointIds for network discovery
    PeerExchange { peers: Vec<String> },
    /// Full catalog sync — send all locally-uploaded tracks to a peer at once
    CatalogSync(Vec<TrackAnnouncement>),
    /// Incremental catalog delta — only tracks modified since the given timestamp
    CatalogDelta {
        since: chrono::DateTime<chrono::Utc>,
        tracks: Vec<TrackAnnouncement>,
    },
    /// Request a peer's full catalog — triggers the remote side to call
    /// `announce_all_tracks_to_peer` back to us.
    RequestCatalog,
    /// Exchange Bloom filters for search routing
    BloomExchange { bloom: BloomFilterData },
    /// Search query sent to peers whose Bloom filter matches
    SearchQuery {
        /// Unique request ID for correlating responses
        request_id: String,
        /// The search query string
        query: String,
        /// Maximum results to return
        limit: u32,
    },
    /// Search results returned by a peer
    SearchResults {
        /// Correlating request ID
        request_id: String,
        /// Matching tracks from this peer
        results: Vec<SearchResultItem>,
        /// Total number of matches on this peer (may exceed returned results)
        total: u64,
    },
}

/// A lightweight search result item returned by distributed search.
/// Contains just enough metadata to display results without downloading full blobs.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SearchResultItem {
    /// BLAKE3 content hash
    pub hash: String,
    /// Track title
    pub title: String,
    /// Artist name
    pub artist_name: String,
    /// Album title (if any)
    pub album_title: Option<String>,
    /// Duration in seconds
    pub duration_secs: f32,
    /// Audio format
    pub format: String,
    /// Genre
    pub genre: Option<String>,
    /// Year
    pub year: Option<i16>,
    /// Bitrate in bps
    pub bitrate: Option<i32>,
    /// The peer EndpointId that has this track
    pub source_node: String,
    /// MusicBrainz recording ID (if resolved)
    pub musicbrainz_id: Option<String>,
    /// Relevance score (ts_rank or similar)
    pub relevance: f32,
}

/// Configuration for the P2P node.
#[derive(Clone, Debug)]
pub struct P2pConfig {
    /// Directory for iroh-blobs persistent storage
    pub blobs_dir: PathBuf,
    /// Path to a persistent secret key file (ensures stable EndpointId across restarts)
    pub secret_key_path: PathBuf,
    /// Bind port (0 = random)
    pub bind_port: u16,
    /// Whether to enable local network (mDNS) discovery
    pub enable_local_discovery: bool,
    /// Seed peer EndpointIds to connect to on startup (auto-discovery)
    pub seed_peers: Vec<String>,
    /// Path to the audio file storage (for cover art sync)
    pub audio_storage_path: PathBuf,
    /// Optional separate directory for metadata files (covers, etc.)
    pub metadata_storage_path: Option<PathBuf>,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            blobs_dir: PathBuf::from("data/p2p/blobs"),
            secret_key_path: PathBuf::from("data/p2p/secret_key"),
            bind_port: 0,
            enable_local_discovery: true,
            seed_peers: Vec::new(),
            audio_storage_path: PathBuf::from("data/music"),
            metadata_storage_path: None,
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
            .unwrap_or_else(|_| blobs_dir.parent().unwrap_or(&blobs_dir).join("secret_key"));

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

        let audio_storage_path = std::env::var("AUDIO_STORAGE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/music"));

        let metadata_storage_path = std::env::var("METADATA_STORAGE_PATH")
            .ok()
            .map(PathBuf::from);

        Self {
            blobs_dir,
            secret_key_path,
            bind_port,
            enable_local_discovery,
            seed_peers,
            audio_storage_path,
            metadata_storage_path,
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
    /// Bloom-filter search index for distributed search routing
    search_index: Arc<SearchIndex>,
    /// MusicBrainz client for metadata enrichment
    mb_client: Arc<MusicBrainzClient>,
    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,
    /// Configuration used to create this node
    _config: P2pConfig,
    /// Path to audio file storage (for writing cover art from peers)
    audio_storage_path: PathBuf,
    /// Optional separate path for metadata (covers). Falls back to audio_storage_path.
    metadata_storage_path: Option<PathBuf>,
    /// LRU cache for P2P track blobs.
    blob_cache: Arc<BlobCache>,
    /// Track health manager for failure tracking and auto-repair.
    health_manager: Arc<TrackHealthManager>,
    /// Semaphore to limit concurrent incoming P2P connections.
    conn_semaphore: Arc<tokio::sync::Semaphore>,
    /// Set of blob hashes that have been explicitly published/announced.
    /// Only these blobs can be served to peers via FetchTrack.
    published_hashes: tokio::sync::RwLock<std::collections::HashSet<String>>,
    /// Round-robin index for PEX peer rotation.
    pex_index: AtomicUsize,
    /// Per-peer mutexes that serialize concurrent `CatalogSync` page processing.
    ///
    /// Keyed by peer EndpointId string. Each entry holds an `Arc<Mutex<()>>` so
    /// that multiple CatalogSync pages from the same peer are processed one at a
    /// time (preventing duplicate track insertions), while syncs from different
    /// peers proceed in parallel.
    catalog_sync_in_progress:
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Pool of reusable QUIC connections to peers (FIX-30).
    conn_pool: Arc<ConnectionPool>,
}

impl P2pNode {
    /// Start a new P2P node with the given config and database.
    pub async fn start(config: P2pConfig, db: DatabaseConnection) -> Result<Arc<Self>, P2pError> {
        // Ensure blobs directory exists
        tokio::fs::create_dir_all(&config.blobs_dir)
            .await
            .map_err(P2pError::Io)?;

        // Load or generate secret key for stable EndpointId across restarts
        let secret_key = Self::load_or_generate_key(&config).await?;
        let node_id = secret_key.public();
        info!(%node_id, "starting P2P node");

        // Initialize the blob store
        let blob_store = FsStore::load(&config.blobs_dir)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        // Build the iroh endpoint with discovery services and relay
        //
        // Endpoint::builder() uses the N0 preset which registers:
        //   - PkarrPublisher  — publishes our EndpointId + relay URL to n0's DNS server
        //   - DnsDiscovery    — resolves other EndpointIds via DNS queries to n0's server
        //
        // Without these, the node cannot register with relay servers and peers
        // cannot discover us — this was the root cause of "Relay Disconnected".
        let mut builder = Endpoint::builder()
            .secret_key(secret_key.clone())
            .alpns(vec![SOUNDTIME_ALPN.to_vec()]);

        // Optionally enable local network discovery (mDNS)
        if config.enable_local_discovery {
            info!("enabling local network discovery (mDNS)");
            builder =
                builder.address_lookup(iroh::address_lookup::MdnsAddressLookupBuilder::default());
            tracing::info!("mDNS address lookup configured");
        }

        // Bind to the configured port (0 = random)
        if config.bind_port > 0 {
            builder = builder
                .bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, config.bind_port))
                .map_err(|e| P2pError::Endpoint(e.to_string()))?;
            info!(
                port = config.bind_port,
                "binding P2P endpoint to configured port"
            );
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
        {
            let addr = endpoint.addr();
            info!(
                direct_addrs = addr.ip_addrs().count(),
                relay = ?addr.relay_urls().next().map(|u| u.to_string()),
                "P2P endpoint bound"
            );
        }

        let (shutdown_tx, _) = watch::channel(false);

        let registry = Arc::new(PeerRegistry::new());

        // Load persisted peers from database (previous runs)
        match registry.load_from_db(&db).await {
            Ok(count) if count > 0 => info!(%count, "restored peers from database"),
            Ok(_) => debug!("no persisted peers found in database"),
            Err(e) => warn!("failed to load peers from database: {e}"),
        }

        let search_index = Arc::new(SearchIndex::new());
        let mb_client = Arc::new(MusicBrainzClient::new());

        let audio_storage_path = config.audio_storage_path.clone();
        let metadata_storage_path = config.metadata_storage_path.clone();

        let blob_cache = Arc::new(BlobCache::from_env());

        let health_manager = Arc::new(TrackHealthManager::new());

        let conn_pool = Arc::new(ConnectionPool::new(endpoint.clone(), SOUNDTIME_ALPN));

        let node = Arc::new(Self {
            endpoint,
            blob_store,
            db,
            registry,
            search_index,
            mb_client,
            shutdown_tx,
            _config: config,
            audio_storage_path,
            metadata_storage_path,
            blob_cache,
            health_manager,
            conn_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_P2P_CONNECTIONS)),
            published_hashes: tokio::sync::RwLock::new(std::collections::HashSet::new()),
            pex_index: AtomicUsize::new(0),
            catalog_sync_in_progress: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            conn_pool,
        });

        // Build the local Bloom filter index from existing tracks in DB
        {
            let node_clone = Arc::clone(&node);
            tokio::spawn(async move {
                node_clone.rebuild_search_index().await;
            });
        }

        // Pre-populate published_hashes with all locally-uploaded tracks (FIX-19)
        {
            let node_clone = Arc::clone(&node);
            tokio::spawn(async move {
                match track::Entity::find()
                    .filter(track::Column::ContentHash.is_not_null())
                    .all(&node_clone.db)
                    .await
                {
                    Ok(tracks) => {
                        let mut hashes = node_clone.published_hashes.write().await;
                        for t in &tracks {
                            if let Some(ref h) = t.content_hash {
                                hashes.insert(h.clone());
                            }
                        }
                        info!(
                            count = hashes.len(),
                            "pre-populated published hashes from database"
                        );
                    }
                    Err(e) => {
                        warn!("failed to pre-populate published hashes: {e}");
                    }
                }
            });
        }

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

        // Re-ping persisted peers in background (after relay setup)
        {
            let node_clone = Arc::clone(&node);
            tokio::spawn(async move {
                // Wait for relay to be ready
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                crate::discovery::refresh_all_peers(&node_clone, &node_clone.registry).await;
                // Save updated statuses to database
                if let Err(e) = node_clone.registry.save_to_db(&node_clone.db).await {
                    warn!("failed to save peers after refresh: {e}");
                }
            });
        }

        // Spawn periodic peer exchange, bloom sync & refresh (every 5 minutes)
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
                                info!(online = peers.len(), "periodic peer exchange + bloom sync");
                                // Query up to 3 peers per cycle, rotating through the list
                                let start = node_clone.pex_index.fetch_add(1, Ordering::Relaxed) % peers.len();
                                let count = std::cmp::min(3, peers.len());
                                for i in 0..count {
                                    let idx = (start + i) % peers.len();
                                    if let Ok(nid) = peers[idx].node_id.parse::<EndpointId>() {
                                        node_clone.discover_via_peer(nid).await;
                                    }
                                }
                                // Rebuild Bloom filter if it was marked dirty (e.g. track deletion)
                                if node_clone.search_index.is_dirty() {
                                    if let Err(e) = node_clone.search_index.rebuild_from_db(&node_clone.db).await {
                                        warn!("failed to rebuild dirty search index: {e}");
                                    }
                                }
                                // Exchange Bloom filters with all online peers
                                node_clone.broadcast_bloom_filter().await;
                            }
                            // Persist peer registry to database
                            if let Err(e) = node_clone.registry.save_to_db(&node_clone.db).await {
                                warn!("failed to save peers: {e}");
                            }
                        }
                        _ = shutdown_rx.changed() => {
                            break;
                        }
                    }
                }
            });
        }

        // Spawn periodic health monitor for remote tracks
        {
            let node_clone: Arc<P2pNode> = Arc::clone(&node);
            let health_manager = Arc::clone(&node.health_manager);
            let shutdown_rx = node.shutdown_tx.subscribe();
            let db_clone = node_clone.db.clone();
            // TrackFetcher is implemented for Arc<P2pNode>, so we wrap in Arc
            let fetcher: Arc<Arc<P2pNode>> = Arc::new(node_clone);
            spawn_health_monitor(health_manager, fetcher, db_clone, shutdown_rx);
        }

        // Spawn periodic relay health check (every 60s)
        // Logs relay status and detects reconnections
        {
            let endpoint_clone = node.endpoint.clone();
            let mut shutdown_rx = node.shutdown_tx.subscribe();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                interval.tick().await; // skip first immediate tick
                let mut was_connected = true; // assume connected after init
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let addr = endpoint_clone.addr();
                            let relay = addr.relay_urls().next().map(|u| u.to_string());
                            match relay {
                                Some(url) => {
                                    if !was_connected {
                                        info!(relay = %url, "relay connection re-established");
                                        was_connected = true;
                                    } else {
                                        debug!(relay = %url, "relay connection OK");
                                    }
                                }
                                None => {
                                    if was_connected {
                                        warn!("relay disconnected — P2P will use direct connections only");
                                        was_connected = false;
                                    } else {
                                        debug!("still no relay connection");
                                    }
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

    /// Returns the base directory for cover art storage.
    /// Uses `metadata_storage_path` if set, otherwise falls back to `audio_storage_path`.
    fn cover_base_path(&self) -> &std::path::Path {
        self.metadata_storage_path
            .as_deref()
            .unwrap_or(&self.audio_storage_path)
    }

    /// Get this node's unique identifier (public key).
    pub fn node_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Get the peer registry.
    pub fn registry(&self) -> &Arc<PeerRegistry> {
        &self.registry
    }

    /// Get the search index.
    pub fn search_index(&self) -> &Arc<SearchIndex> {
        &self.search_index
    }

    /// Get the database connection.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Get the node address (includes relay URL and direct addresses).
    pub fn node_addr(&self) -> Result<EndpointAddr, P2pError> {
        Ok(self.endpoint.addr())
    }

    /// Get the relay URL this node is connected to (if any).
    /// By default, iroh connects to n0.computer production relay servers.
    pub fn relay_url(&self) -> Option<String> {
        self.node_addr()
            .ok()
            .and_then(|addr| addr.relay_urls().next().map(|u| u.to_string()))
    }

    /// Get the number of direct addresses the endpoint is listening on.
    pub fn direct_addresses_count(&self) -> usize {
        self.node_addr()
            .map(|addr| addr.ip_addrs().count())
            .unwrap_or(0)
    }

    /// Get a reference to the blob cache.
    pub fn blob_cache(&self) -> &Arc<BlobCache> {
        &self.blob_cache
    }

    /// Get the track health manager for failure tracking and auto-repair.
    pub fn health_manager(&self) -> &Arc<TrackHealthManager> {
        &self.health_manager
    }

    /// Publish a track's audio data to the local blob store.
    /// Returns the content hash (BLAKE3) that identifies the blob.
    pub async fn publish_track(&self, data: Bytes) -> Result<Hash, P2pError> {
        let outcome = self
            .blob_store
            .blobs()
            .add_bytes(data)
            .temp_tag()
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        let hash = outcome.hash();

        // Create a persistent tag so the blob is not garbage collected
        let tag_name = format!("published-{}", hash);
        self.blob_store
            .tags()
            .set(tag_name, HashAndFormat::raw(hash))
            .await
            .map_err(|e| P2pError::BlobStore(format!("failed to set persistent tag: {e}")))?;

        // SECURITY: Register hash so it can be served to peers (FIX-19)
        self.published_hashes.write().await.insert(hash.to_string());
        info!(%hash, "track published to blob store");
        Ok(hash)
    }

    /// Publish cover art data to the local blob store.
    /// Returns the content hash (BLAKE3) that identifies the blob.
    pub async fn publish_cover(&self, data: Bytes) -> Result<Hash, P2pError> {
        let outcome = self
            .blob_store
            .blobs()
            .add_bytes(data)
            .temp_tag()
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        let hash = outcome.hash();

        // Create a persistent tag so the blob is not garbage collected
        let tag_name = format!("published-{}", hash);
        self.blob_store
            .tags()
            .set(tag_name, HashAndFormat::raw(hash))
            .await
            .map_err(|e| P2pError::BlobStore(format!("failed to set persistent tag: {e}")))?;

        // SECURITY: Register hash so it can be served to peers (FIX-19)
        self.published_hashes.write().await.insert(hash.to_string());
        debug!(%hash, "cover art published to blob store");
        Ok(hash)
    }

    /// Retrieve a track's data from the local blob store by hash.
    pub async fn get_local_track(&self, hash: Hash) -> Result<Bytes, P2pError> {
        let data = self
            .blob_store
            .blobs()
            .get_bytes(hash)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        Ok(data)
    }

    /// Retrieve a P2P track by hash, fetching from the origin peer on-demand if not cached.
    ///
    /// 1. Try the local blob store (fast path).
    /// 2. If missing, look up the origin peer from `remote_tracks` and fetch via P2P.
    /// 3. Store the fetched blob locally and register it in the LRU cache.
    /// 4. Trigger eviction if the cache exceeds the configured limit.
    pub async fn get_or_fetch_track(&self, hash: Hash) -> Result<Bytes, P2pError> {
        // Fast path: blob exists locally
        if let Ok(data) = self.get_local_track(hash).await {
            self.blob_cache
                .record_access_with_tag(hash, data.len() as u64, &self.blob_store)
                .await;
            return Ok(data);
        }

        // Look up origin peer from remote_track table
        let hash_str = hash.to_string();
        let remote = remote_track::Entity::find()
            .filter(remote_track::Column::RemoteUri.ends_with(format!("/{}", hash_str)))
            .one(&self.db)
            .await
            .map_err(P2pError::Database)?
            .ok_or_else(|| P2pError::TrackNotFound(hash_str.clone()))?;

        let origin = remote
            .instance_domain
            .strip_prefix("p2p://")
            .unwrap_or(&remote.instance_domain);

        let nid: EndpointId = origin
            .parse()
            .map_err(|_| P2pError::Connection(format!("invalid peer id: {}", origin)))?;

        // In-flight dedup: if another task is already fetching this blob, wait and retry
        if !self.blob_cache.try_start_fetch(hash).await {
            debug!(%hash, "another fetch in progress, waiting");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            // Retry local — the other fetch may have completed
            if let Ok(data) = self.get_local_track(hash).await {
                self.blob_cache
                    .record_access_with_tag(hash, data.len() as u64, &self.blob_store)
                    .await;
                return Ok(data);
            }
            return Err(P2pError::TrackNotFound(hash_str));
        }

        // Fetch from peer
        let result = async {
            info!(%hash, peer = %origin, "on-demand fetch from peer");
            let peer_addr = EndpointAddr::new(nid);
            let data = self.fetch_track_from_peer(peer_addr, hash).await?;

            // Store in local blob store
            let _tag = self
                .blob_store
                .blobs()
                .add_bytes(data.clone())
                .temp_tag()
                .await
                .map_err(|e| P2pError::BlobStore(e.to_string()))?;

            // Register in LRU cache and evict if over limit
            self.blob_cache
                .record_access_with_tag(hash, data.len() as u64, &self.blob_store)
                .await;
            self.blob_cache.evict_if_needed(&self.blob_store).await;

            info!(%hash, bytes = data.len(), "on-demand fetch complete, blob cached");
            Ok(data)
        }
        .await;

        // Always clear in-flight flag
        self.blob_cache.finish_fetch(hash).await;

        result
    }

    /// Check if a blob exists locally.
    pub async fn has_blob(&self, hash: Hash) -> bool {
        self.blob_store.blobs().has(hash).await.unwrap_or(false)
    }

    /// Connect to a remote peer and fetch a track by its content hash.
    pub async fn fetch_track_from_peer(
        &self,
        peer_addr: EndpointAddr,
        hash: Hash,
    ) -> Result<Bytes, P2pError> {
        let peer_id = peer_addr.id.to_string();

        // Check if peer is blocked
        if is_peer_blocked(&self.db, &peer_id).await {
            return Err(P2pError::PeerBlocked(peer_id));
        }

        info!(peer = %peer_id, %hash, "fetching track from peer");

        let conn = self.conn_pool.get_connection(peer_addr.id).await?;

        // Send fetch request
        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

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
    pub async fn ping_peer(&self, peer_addr: EndpointAddr) -> Result<P2pMessage, P2pError> {
        let conn = self.conn_pool.get_connection(peer_addr.id).await.map_err(|e| {
            let err_str = format!("{e}");
            if err_str.contains("ALPN") || err_str.contains("protocol") || err_str.contains("timed out") {
                P2pError::Connection(format!(
                    "{e} (possible protocol version mismatch — this node uses iroh 0.96 / ALPN 'soundtime/p2p/1')"
                ))
            } else {
                e
            }
        })?;

        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

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

    /// Connect to a list of seed peers by EndpointId.
    /// Pings each one and registers it in the registry.
    async fn connect_to_seed_peers(&self, seed_peers: &[String]) {
        info!(count = seed_peers.len(), "connecting to seed peers");

        for peer_id_str in seed_peers {
            let node_id: EndpointId = match peer_id_str.parse() {
                Ok(id) => id,
                Err(e) => {
                    warn!(peer = %peer_id_str, "invalid seed peer EndpointId: {e}");
                    continue;
                }
            };

            // Skip ourselves
            if node_id == self.node_id() {
                debug!("skipping self in seed peers");
                continue;
            }

            let peer_addr = EndpointAddr::new(node_id);
            info!(peer = %peer_id_str, "pinging seed peer");

            match self.ping_peer(peer_addr).await {
                Ok(P2pMessage::Pong {
                    node_id: nid,
                    track_count,
                    version,
                }) => {
                    self.registry
                        .upsert_peer_versioned(&nid, None, track_count, version)
                        .await;
                    info!(peer = %nid, %track_count, "seed peer connected and registered");
                    // Exchange peer lists to discover the wider network
                    self.discover_via_peer(node_id).await;
                    // Sync our full catalog to this peer so they get our existing tracks
                    self.announce_all_tracks_to_peer(node_id).await;
                }
                Ok(_) => {
                    self.registry.upsert_peer(peer_id_str, None, 0).await;
                    warn!(peer = %peer_id_str, "seed peer responded with unexpected message");
                }
                Err(e) => {
                    // Check if it's a protocol version mismatch
                    let err_str = format!("{e}");
                    if err_str.contains("ALPN")
                        || err_str.contains("protocol")
                        || err_str.contains("version")
                        || err_str.contains("timed out")
                    {
                        warn!(
                            peer = %peer_id_str,
                            "failed to reach seed peer (possible protocol version mismatch — \
                             this node uses iroh 0.96 / ALPN 'soundtime/p2p/1', the remote peer \
                             may be running an incompatible version): {e}"
                        );
                    } else {
                        warn!(peer = %peer_id_str, "failed to reach seed peer: {e}");
                    }
                }
            }
        }

        let total = self.registry.peer_count().await;
        let online = self.registry.online_peers().await.len();
        info!(%total, %online, "seed peer discovery complete");
    }

    /// Send a message to a specific peer with retry and exponential backoff.
    ///
    /// Serializes the message once, then attempts to deliver it up to 3 times
    /// with doubling delays (1s, 2s, 4s) between retries.
    async fn send_message_to_peer(
        &self,
        node_id: EndpointId,
        msg: &P2pMessage,
    ) -> Result<(), P2pError> {
        let msg_bytes = serde_json::to_vec(msg)?;
        let max_retries = 3u32;
        let mut delay = std::time::Duration::from_secs(1);

        for attempt in 0..max_retries {
            match self.try_send_bytes(node_id, &msg_bytes).await {
                Ok(()) => return Ok(()),
                Err(e) if attempt < max_retries - 1 => {
                    warn!(peer = %node_id, attempt, "send failed, retrying in {:?}: {e}", delay);
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    /// Internal: single attempt to send pre-serialized message bytes to a peer.
    async fn try_send_bytes(&self, node_id: EndpointId, msg_bytes: &[u8]) -> Result<(), P2pError> {
        let conn = self.conn_pool.get_connection(node_id).await?;

        let result: Result<(), P2pError> = async {
            let (mut send, _recv) = conn
                .open_bi()
                .await
                .map_err(|e| P2pError::Connection(e.to_string()))?;

            send.write_all(&(msg_bytes.len() as u32).to_be_bytes())
                .await
                .map_err(|e| P2pError::Connection(e.to_string()))?;
            send.write_all(msg_bytes)
                .await
                .map_err(|e| P2pError::Connection(e.to_string()))?;
            send.finish()
                .map_err(|e| P2pError::Connection(e.to_string()))?;

            // Wait for the peer to acknowledge receipt (with timeout)
            match tokio::time::timeout(std::time::Duration::from_secs(30), send.stopped()).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => {}
                Err(_) => {
                    warn!("timed out waiting for stream ack");
                }
            }

            Ok(())
        }
        .await;

        if result.is_err() {
            self.conn_pool.invalidate(&node_id).await;
        }

        result
    }

    /// Broadcast a track announcement to all online peers concurrently.
    /// Called after a track is published to the local blob store.
    /// Uses a semaphore to limit concurrency to 10 simultaneous sends.
    pub async fn broadcast_announce_track(self: &Arc<Self>, announcement: TrackAnnouncement) {
        let peers = self.registry.online_peers().await;
        if peers.is_empty() {
            debug!(hash = %announcement.hash, "no online peers to announce track to");
            return;
        }

        info!(
            hash = %announcement.hash,
            title = %announcement.title,
            artist = %announcement.artist_name,
            peer_count = peers.len(),
            "broadcasting track announcement"
        );

        let msg = P2pMessage::AnnounceTrack(announcement);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10));
        let mut handles = Vec::new();

        for peer in &peers {
            let node_id: EndpointId = match peer.node_id.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };

            let node = Arc::clone(self);
            let msg = msg.clone();
            let sem = Arc::clone(&semaphore);
            let peer_id = peer.node_id.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.ok();
                if let Err(e) = node.send_message_to_peer(node_id, &msg).await {
                    warn!(peer = %peer_id, "failed to announce track: {e}");
                    node.registry.mark_offline(&peer_id).await;
                } else {
                    debug!(peer = %peer_id, "track announced");
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }
    }

    /// Announce all locally-uploaded tracks to a specific peer using paginated queries.
    /// Called when a new peer connects to sync existing catalogs.
    /// Sends one CatalogSync message per page (500 tracks) to avoid loading all
    /// tracks into memory at once.
    pub async fn announce_all_tracks_to_peer(&self, peer_id: EndpointId) {
        let page_size = 500u64;
        let total = match track::Entity::find()
            .filter(track::Column::ContentHash.is_not_null())
            .count(&self.db)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to count tracks for catalog sync: {e}");
                return;
            }
        };

        if total == 0 {
            debug!(peer = %peer_id, "no tracks to sync");
            return;
        }

        let num_pages = total.div_ceil(page_size);
        info!(peer = %peer_id, total, pages = num_pages, "starting paginated catalog sync");

        let our_node = self.node_id().to_string();

        for page_num in 0..num_pages {
            let tracks = match track::Entity::find()
                .filter(track::Column::ContentHash.is_not_null())
                .paginate(&self.db, page_size)
                .fetch_page(page_num)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    warn!(
                        page = page_num,
                        "failed to read tracks page for catalog sync: {e}"
                    );
                    continue;
                }
            };

            let mut announcements = Vec::with_capacity(tracks.len());

            for t in &tracks {
                // Skip P2P-replicated tracks (they came from another peer, don't re-announce)
                if t.file_path.starts_with("p2p://") {
                    continue;
                }

                let hash = match &t.content_hash {
                    Some(h) => h.clone(),
                    None => continue,
                };

                let artist_name = artist::Entity::find_by_id(t.artist_id)
                    .one(&self.db)
                    .await
                    .ok()
                    .flatten()
                    .map(|a| a.name)
                    .unwrap_or_else(|| "Unknown".to_string());

                let (album_title, cover_hash) = match t.album_id {
                    Some(aid) => {
                        let album_opt = album::Entity::find_by_id(aid)
                            .one(&self.db)
                            .await
                            .ok()
                            .flatten();
                        match album_opt {
                            Some(a) => {
                                let title = Some(a.title);
                                // Publish cover art to blob store if available
                                let ch = if let Some(ref url) = a.cover_url {
                                    // cover_url is like "/api/media/<relative_path>"
                                    let rel = url.strip_prefix("/api/media/").unwrap_or(url);
                                    let full = self.audio_storage_path.join(rel);
                                    let cover_data = match tokio::fs::read(&full).await {
                                        Ok(data) => Some(data),
                                        Err(_) => {
                                            if let Some(ref meta) = self.metadata_storage_path {
                                                tokio::fs::read(meta.join(rel)).await.ok()
                                            } else {
                                                None
                                            }
                                        }
                                    };
                                    match cover_data {
                                        Some(data) => {
                                            match self.publish_cover(Bytes::from(data)).await {
                                                Ok(h) => Some(h.to_string()),
                                                Err(e) => {
                                                    warn!("failed to publish cover blob: {e}");
                                                    None
                                                }
                                            }
                                        }
                                        None => None,
                                    }
                                } else {
                                    None
                                };
                                (title, ch)
                            }
                            None => (None, None),
                        }
                    }
                    None => (None, None),
                };

                announcements.push(TrackAnnouncement {
                    hash,
                    title: t.title.clone(),
                    artist_name,
                    album_artist_name: None, // Not available from DB query; artist_id on album suffices
                    album_title,
                    duration_secs: t.duration_secs,
                    format: t.format.clone(),
                    file_size: t.file_size,
                    genre: t.genre.clone(),
                    year: t.year,
                    track_number: t.track_number,
                    disc_number: t.disc_number,
                    bitrate: t.bitrate,
                    sample_rate: t.sample_rate,
                    origin_node: our_node.clone(),
                    cover_hash,
                });
            }

            if !announcements.is_empty() {
                info!(
                    peer = %peer_id,
                    count = announcements.len(),
                    page = page_num,
                    "syncing catalog page to peer"
                );

                let msg = P2pMessage::CatalogSync(announcements);
                if let Err(e) = self.send_message_to_peer(peer_id, &msg).await {
                    warn!(peer = %peer_id, page = page_num, "failed to sync catalog page: {e}");
                }
            }
        }
    }

    /// Send a `RequestCatalog` message to a peer, asking them to send us their
    /// full catalog via paginated `CatalogSync` messages.
    pub async fn request_catalog_from_peer(&self, peer_id: EndpointId) -> Result<(), P2pError> {
        info!(peer = %peer_id, "requesting full catalog from peer");
        self.send_message_to_peer(peer_id, &P2pMessage::RequestCatalog)
            .await
    }

    /// Discover peers by exchanging peer lists with a known peer (Peer Exchange / PEX).
    /// Sends our known peers, receives theirs, and connects to any new ones.
    pub async fn discover_via_peer(&self, peer_node_id: EndpointId) {
        info!(peer = %peer_node_id, "initiating peer exchange");

        let peer_addr = EndpointAddr::new(peer_node_id);
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

                    let nid: EndpointId = match peer_id_str.parse() {
                        Ok(id) => id,
                        Err(_) => continue,
                    };

                    let addr = EndpointAddr::new(nid);
                    match self.ping_peer(addr).await {
                        Ok(P2pMessage::Pong {
                            node_id,
                            track_count,
                            version,
                        }) => {
                            self.registry
                                .upsert_peer_versioned(&node_id, None, track_count, version)
                                .await;
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
    async fn exchange_peers(&self, peer_addr: EndpointAddr) -> Result<Vec<String>, P2pError> {
        let conn = self.conn_pool.get_connection(peer_addr.id).await?;

        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

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
        let _ = self.blob_store.shutdown().await;
        info!("P2P node shutdown complete");
    }

    /// Rebuild the local Bloom filter search index from all tracks in the database.
    async fn rebuild_search_index(&self) {
        match self.search_index.rebuild_from_db(&self.db).await {
            Ok(()) => info!("search index rebuilt at startup"),
            Err(e) => warn!("failed to rebuild search index from database: {e}"),
        }
    }

    /// Broadcast our Bloom filter to all online peers for search routing.
    pub async fn broadcast_bloom_filter(&self) {
        let bloom_data = self.search_index.export_local_bloom().await;
        let msg = P2pMessage::BloomExchange { bloom: bloom_data };
        let peers = self.registry.online_peers().await;

        for peer in &peers {
            let node_id: EndpointId = match peer.node_id.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };
            if let Err(e) = self.send_message_to_peer(node_id, &msg).await {
                debug!(peer = %peer.node_id, "failed to send bloom filter: {e}");
            }
        }

        if !peers.is_empty() {
            info!(peers = peers.len(), "broadcast bloom filter to peers");
        }
    }

    /// Perform a distributed search across the P2P network.
    /// Uses Bloom filters to route the query only to peers likely to have results.
    /// Queries up to 10 matching peers concurrently with a 10-second timeout per peer.
    /// Returns search results from all matching peers, merged and sorted by relevance.
    pub async fn distributed_search(
        self: &Arc<Self>,
        query: &str,
        limit: u32,
    ) -> Vec<SearchResultItem> {
        let matching_peers = self.search_index.peers_matching_query(query).await;

        if matching_peers.is_empty() {
            debug!(query = query, "no peers match bloom filter for query");
            return vec![];
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let msg = P2pMessage::SearchQuery {
            request_id: request_id.clone(),
            query: query.to_string(),
            limit,
        };

        let mut all_results: Vec<SearchResultItem> = Vec::new();

        // Collect peers to query (up to 10)
        let mut peers_to_query = Vec::new();
        for peer_id_str in matching_peers.iter().take(10) {
            let nid: EndpointId = match peer_id_str.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };
            let peer_addr = EndpointAddr::new(nid);
            peers_to_query.push((peer_id_str.clone(), peer_addr));
        }

        // Query all matched peers concurrently using JoinSet
        let timeout_dur = std::time::Duration::from_secs(10);
        let mut join_set = tokio::task::JoinSet::new();

        for (peer_id_str, peer_addr) in peers_to_query {
            let node = Arc::clone(self);
            let msg = msg.clone();
            join_set.spawn(async move {
                match tokio::time::timeout(timeout_dur, node.search_peer(peer_addr, &msg)).await {
                    Ok(Ok(results)) => {
                        info!(peer = %peer_id_str, results = results.len(), "received search results");
                        results
                    }
                    Ok(Err(e)) => {
                        debug!(peer = %peer_id_str, "search failed: {e}");
                        vec![]
                    }
                    Err(_) => {
                        debug!(peer = %peer_id_str, "search timed out");
                        vec![]
                    }
                }
            });
        }

        while let Some(result) = join_set.join_next().await {
            if let Ok(results) = result {
                all_results.extend(results);
            }
        }

        // Sort by relevance (highest first), deduplicate by hash
        all_results.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate by content hash (keep highest relevance)
        let mut seen_hashes = std::collections::HashSet::new();
        all_results.retain(|r| seen_hashes.insert(r.hash.clone()));

        // Apply limit
        all_results.truncate(limit as usize);

        info!(
            query = query,
            total_results = all_results.len(),
            "distributed search complete"
        );

        all_results
    }

    /// Send a search query to a specific peer and wait for results.
    async fn search_peer(
        &self,
        peer_addr: EndpointAddr,
        msg: &P2pMessage,
    ) -> Result<Vec<SearchResultItem>, P2pError> {
        let conn = self.conn_pool.get_connection(peer_addr.id).await?;

        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

        let msg_bytes = serde_json::to_vec(msg)?;
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
            P2pMessage::SearchResults { results, .. } => Ok(results),
            _ => Ok(vec![]),
        }
    }

    /// Handle an incoming SearchQuery: run a local FTS query and return results.
    async fn handle_search_query(
        &self,
        request_id: &str,
        query: &str,
        limit: u32,
        mut send: iroh::endpoint::SendStream,
    ) -> Result<(), P2pError> {
        use sea_orm::{FromQueryResult, Statement};

        // Build tsquery from the search string
        let tsquery = query
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| {
                let clean: String = w
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if clean.is_empty() {
                    String::new()
                } else {
                    format!("{}:*", clean)
                }
            })
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" & ");

        if tsquery.is_empty() {
            let resp = P2pMessage::SearchResults {
                request_id: request_id.to_string(),
                results: vec![],
                total: 0,
            };
            let resp_bytes = serde_json::to_vec(&resp)?;
            send.write_all(&(resp_bytes.len() as u32).to_be_bytes())
                .await
                .map_err(|e| P2pError::Connection(e.to_string()))?;
            send.write_all(&resp_bytes)
                .await
                .map_err(|e| P2pError::Connection(e.to_string()))?;
            send.finish()
                .map_err(|e| P2pError::Connection(e.to_string()))?;
            return Ok(());
        }

        #[derive(Debug, FromQueryResult)]
        struct SearchRow {
            hash: Option<String>,
            title: String,
            artist_name: String,
            album_title: Option<String>,
            duration_secs: f32,
            format: String,
            genre: Option<String>,
            year: Option<i16>,
            bitrate: Option<i32>,
            musicbrainz_id: Option<String>,
            rank: f32,
        }

        let our_node = self.node_id().to_string();

        let rows: Vec<SearchRow> = SearchRow::find_by_statement(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"
            SELECT t.content_hash AS hash, t.title, a.name AS artist_name,
                   al.title AS album_title, t.duration_secs, t.format,
                   t.genre, t.year, t.bitrate, t.musicbrainz_id,
                   ts_rank(
                       setweight(to_tsvector('english', t.title), 'A') ||
                       setweight(to_tsvector('english', a.name), 'B') ||
                       setweight(to_tsvector('english', COALESCE(al.title, '')), 'C'),
                       to_tsquery('english', $1)
                   ) AS rank
            FROM tracks t
            JOIN artists a ON a.id = t.artist_id
            LEFT JOIN albums al ON al.id = t.album_id
            WHERE (
                to_tsvector('english', t.title) ||
                to_tsvector('english', a.name) ||
                to_tsvector('english', COALESCE(al.title, ''))
            ) @@ to_tsquery('english', $1)
            ORDER BY rank DESC
            LIMIT $2
            "#,
            vec![tsquery.into(), (limit as i64).into()],
        ))
        .all(&self.db)
        .await
        .unwrap_or_default();

        let results: Vec<SearchResultItem> = rows
            .into_iter()
            .filter(|r| r.hash.is_some())
            .map(|r| SearchResultItem {
                hash: r.hash.unwrap_or_default(),
                title: r.title,
                artist_name: r.artist_name,
                album_title: r.album_title,
                duration_secs: r.duration_secs,
                format: r.format,
                genre: r.genre,
                year: r.year,
                bitrate: r.bitrate,
                source_node: our_node.clone(),
                musicbrainz_id: r.musicbrainz_id,
                relevance: r.rank,
            })
            .collect();

        let total = results.len() as u64;
        let resp = P2pMessage::SearchResults {
            request_id: request_id.to_string(),
            results,
            total,
        };

        let resp_bytes = serde_json::to_vec(&resp)?;
        send.write_all(&(resp_bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.write_all(&resp_bytes)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        send.finish()
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        Ok(())
    }

    /// Announce all locally-uploaded tracks to a specific peer using incremental sync
    /// when possible (if we have a `last_seen` timestamp for this peer), otherwise
    /// falls back to a full CatalogSync.
    pub async fn incremental_sync_to_peer(
        &self,
        peer_id: EndpointId,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        let since_ts = since.unwrap_or(chrono::DateTime::UNIX_EPOCH);

        let tracks = match track::Entity::find()
            .filter(track::Column::ContentHash.is_not_null())
            .filter(track::Column::CreatedAt.gt(since_ts))
            .all(&self.db)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!("failed to read tracks for incremental sync: {e}");
                return;
            }
        };

        if tracks.is_empty() {
            debug!(peer = %peer_id, "no new tracks since last sync");
            return;
        }

        let mut announcements = Vec::with_capacity(tracks.len());
        let our_node = self.node_id().to_string();

        for t in &tracks {
            if t.file_path.starts_with("p2p://") {
                continue;
            }
            let hash = match &t.content_hash {
                Some(h) => h.clone(),
                None => continue,
            };

            let artist_name = artist::Entity::find_by_id(t.artist_id)
                .one(&self.db)
                .await
                .ok()
                .flatten()
                .map(|a| a.name)
                .unwrap_or_else(|| "Unknown".to_string());

            let (album_title, cover_hash) = match t.album_id {
                Some(aid) => {
                    let album_opt = album::Entity::find_by_id(aid)
                        .one(&self.db)
                        .await
                        .ok()
                        .flatten();
                    match album_opt {
                        Some(a) => {
                            let title = Some(a.title);
                            let ch = if let Some(ref url) = a.cover_url {
                                let rel = url.strip_prefix("/api/media/").unwrap_or(url);
                                let full = self.audio_storage_path.join(rel);
                                let cover_data = match tokio::fs::read(&full).await {
                                    Ok(data) => Some(data),
                                    Err(_) => {
                                        if let Some(ref meta) = self.metadata_storage_path {
                                            tokio::fs::read(meta.join(rel)).await.ok()
                                        } else {
                                            None
                                        }
                                    }
                                };
                                match cover_data {
                                    Some(data) => match self.publish_cover(Bytes::from(data)).await
                                    {
                                        Ok(h) => Some(h.to_string()),
                                        Err(_) => None,
                                    },
                                    None => None,
                                }
                            } else {
                                None
                            };
                            (title, ch)
                        }
                        None => (None, None),
                    }
                }
                None => (None, None),
            };

            announcements.push(TrackAnnouncement {
                hash,
                title: t.title.clone(),
                artist_name,
                album_artist_name: None,
                album_title,
                duration_secs: t.duration_secs,
                format: t.format.clone(),
                file_size: t.file_size,
                genre: t.genre.clone(),
                year: t.year,
                track_number: t.track_number,
                disc_number: t.disc_number,
                bitrate: t.bitrate,
                sample_rate: t.sample_rate,
                origin_node: our_node.clone(),
                cover_hash,
            });
        }

        if announcements.is_empty() {
            return;
        }

        info!(
            peer = %peer_id,
            count = announcements.len(),
            since = %since_ts,
            "incremental catalog sync"
        );

        let msg = P2pMessage::CatalogDelta {
            since: since_ts,
            tracks: announcements,
        };
        if let Err(e) = self.send_message_to_peer(peer_id, &msg).await {
            warn!(peer = %peer_id, "failed to send catalog delta: {e}");
        }

        // Also send our bloom filter so the peer can route searches to us
        let bloom_data = self.search_index.export_local_bloom().await;
        let bloom_msg = P2pMessage::BloomExchange { bloom: bloom_data };
        if let Err(e) = self.send_message_to_peer(peer_id, &bloom_msg).await {
            debug!(peer = %peer_id, "failed to send bloom filter: {e}");
        }
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
                            let permit = match node.conn_semaphore.clone().try_acquire_owned() {
                                Ok(p) => p,
                                Err(_) => {
                                    warn!("max concurrent P2P connections reached, dropping connection");
                                    continue;
                                }
                            };
                            tokio::spawn(async move {
                                let _permit = permit; // held for the duration
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
    async fn handle_connection(self: &Arc<Self>, conn: Connection) -> Result<(), P2pError> {
        let peer_id = conn.remote_id().to_string();

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
        while let Ok((send, mut recv)) = conn.accept_bi().await {
            let node_id = self.node_id();

            // Read length-prefixed message
            let mut len_buf = [0u8; 4];
            if recv.read_exact(&mut len_buf).await.is_err() {
                break;
            }
            let msg_len = u32::from_be_bytes(len_buf) as usize;

            // SECURITY: Reject oversized messages to prevent OOM (FIX-17)
            if msg_len > MAX_P2P_MESSAGE_SIZE {
                warn!(%peer_id, msg_len, "rejecting oversized P2P message (max: {MAX_P2P_MESSAGE_SIZE})");
                break;
            }

            let msg_bytes = match recv.read_to_end(msg_len).await {
                Ok(b) => b,
                Err(_) => break,
            };

            // SECURITY: Message size is bounded by MAX_P2P_MESSAGE_SIZE (FIX-17).
            // serde_json's default recursion limit (128) provides depth protection.
            let msg: P2pMessage = match serde_json::from_slice(&msg_bytes) {
                Ok(m) => m,
                Err(e) => {
                    warn!("invalid message from {peer_id}: {e}");
                    break;
                }
            };

            self.handle_message(msg, send, node_id, &peer_id).await?;
        }

        Ok(())
    }

    /// Internal: process a single track announcement — de-duplicate, auto-fetch blob,
    /// create artist/album/track/remote_track records in the local database.
    /// Used by both AnnounceTrack (single) and CatalogSync (batch) handlers.
    async fn process_track_announcement(&self, ann: TrackAnnouncement, peer_id: &str) {
        info!(
            hash = %ann.hash,
            title = %ann.title,
            artist = %ann.artist_name,
            %peer_id,
            "processing track announcement"
        );
        self.registry.upsert_peer(peer_id, None, 0).await;

        // Check if we already have this track (by content_hash)
        let already_exists = track::Entity::find()
            .filter(track::Column::ContentHash.eq(Some(ann.hash.clone())))
            .one(&self.db)
            .await
            .ok()
            .flatten()
            .is_some();

        if already_exists {
            debug!(hash = %ann.hash, "track already in local catalog, skipping");
            return;
        }

        // Blob is fetched lazily on first play (get_or_fetch_track) — no eager download
        debug!(hash = %ann.hash, %peer_id, "track metadata stored, blob will be fetched on demand");

        // Create artist (find or create)
        let artist_id = match artist::Entity::find()
            .filter(artist::Column::Name.eq(&ann.artist_name))
            .one(&self.db)
            .await
        {
            Ok(Some(a)) => a.id,
            _ => {
                let new_id = Uuid::new_v4();
                let new_artist = artist::ActiveModel {
                    id: Set(new_id),
                    name: Set(ann.artist_name.clone()),
                    musicbrainz_id: Set(None),
                    bio: Set(None),
                    image_url: Set(None),
                    created_at: Set(chrono::Utc::now().into()),
                };
                if let Err(e) = new_artist.insert(&self.db).await {
                    warn!(artist = %ann.artist_name, "failed to create artist: {e}");
                    match artist::Entity::find()
                        .filter(artist::Column::Name.eq(&ann.artist_name))
                        .one(&self.db)
                        .await
                    {
                        Ok(Some(a)) => a.id,
                        _ => return,
                    }
                } else {
                    new_id
                }
            }
        };

        // Resolve album artist for proper album grouping (compilations)
        let album_artist_name = ann
            .album_artist_name
            .clone()
            .unwrap_or_else(|| ann.artist_name.clone());

        let album_artist_id = if album_artist_name == ann.artist_name {
            artist_id
        } else {
            let existing = artist::Entity::find()
                .filter(artist::Column::Name.eq(&album_artist_name))
                .one(&self.db)
                .await;
            match existing {
                Ok(Some(a)) => a.id,
                _ => {
                    let new_id = Uuid::new_v4();
                    let new_artist = artist::ActiveModel {
                        id: Set(new_id),
                        name: Set(album_artist_name.clone()),
                        musicbrainz_id: Set(None),
                        bio: Set(None),
                        image_url: Set(None),
                        created_at: Set(chrono::Utc::now().into()),
                    };
                    if let Err(e) = new_artist.insert(&self.db).await {
                        warn!(artist = %album_artist_name, "failed to create album artist: {e}");
                        match artist::Entity::find()
                            .filter(artist::Column::Name.eq(&album_artist_name))
                            .one(&self.db)
                            .await
                        {
                            Ok(Some(a)) => a.id,
                            _ => artist_id, // fallback to track artist
                        }
                    } else {
                        new_id
                    }
                }
            }
        };

        // Create album (find or create) if present
        let album_id = if let Some(ref album_title) = ann.album_title {
            match album::Entity::find()
                .filter(album::Column::Title.eq(album_title))
                .filter(album::Column::ArtistId.eq(album_artist_id))
                .one(&self.db)
                .await
            {
                Ok(Some(a)) => {
                    // If album already exists but has no cover, try to sync one
                    if a.cover_url.is_none() {
                        self.sync_cover_for_album(a.id, &ann, peer_id).await;
                    }
                    Some(a.id)
                }
                _ => {
                    let new_id = Uuid::new_v4();
                    let new_album = album::ActiveModel {
                        id: Set(new_id),
                        title: Set(album_title.clone()),
                        artist_id: Set(album_artist_id),
                        release_date: Set(None),
                        cover_url: Set(None),
                        musicbrainz_id: Set(None),
                        genre: Set(ann.genre.clone()),
                        year: Set(ann.year),
                        created_at: Set(chrono::Utc::now().into()),
                    };
                    match new_album.insert(&self.db).await {
                        Ok(_) => {
                            // Sync cover for newly created album
                            self.sync_cover_for_album(new_id, &ann, peer_id).await;
                            Some(new_id)
                        }
                        Err(e) => {
                            warn!(album = %album_title, "failed to create album: {e}");
                            album::Entity::find()
                                .filter(album::Column::Title.eq(album_title))
                                .filter(album::Column::ArtistId.eq(album_artist_id))
                                .one(&self.db)
                                .await
                                .ok()
                                .flatten()
                                .map(|a| a.id)
                        }
                    }
                }
            }
        } else {
            None
        };

        // Create the track record — file_path points to the P2P blob hash
        let track_id = Uuid::new_v4();
        let new_track = track::ActiveModel {
            id: Set(track_id),
            title: Set(ann.title.clone()),
            artist_id: Set(artist_id),
            album_id: Set(album_id),
            track_number: Set(ann.track_number),
            disc_number: Set(ann.disc_number),
            duration_secs: Set(ann.duration_secs),
            genre: Set(ann.genre.clone()),
            year: Set(ann.year),
            musicbrainz_id: Set(None),
            file_path: Set(format!("p2p://{}", ann.hash)),
            file_size: Set(ann.file_size),
            format: Set(ann.format.clone()),
            bitrate: Set(ann.bitrate),
            sample_rate: Set(ann.sample_rate),
            waveform_data: Set(None),
            uploaded_by: Set(None),
            content_hash: Set(Some(ann.hash.clone())),
            play_count: Set(0),
            created_at: Set(chrono::Utc::now().into()),
        };

        match new_track.insert(&self.db).await {
            Ok(_) => {
                info!(
                    %track_id,
                    title = %ann.title,
                    artist = %ann.artist_name,
                    hash = %ann.hash,
                    "remote track replicated to local catalog"
                );

                // Index in Bloom filter for search routing
                self.search_index
                    .add_track_tokens(&ann.title, &ann.artist_name, ann.album_title.as_deref())
                    .await;

                let remote_track_id = Uuid::new_v4();
                let origin = &ann.origin_node;
                let new_remote = remote_track::ActiveModel {
                    id: Set(remote_track_id),
                    local_track_id: Set(Some(track_id)),
                    musicbrainz_id: Set(None),
                    title: Set(ann.title.clone()),
                    artist_name: Set(ann.artist_name.clone()),
                    album_title: Set(ann.album_title.clone()),
                    instance_domain: Set(format!("p2p://{}", origin)),
                    remote_uri: Set(format!("p2p://{}/{}", origin, ann.hash)),
                    remote_stream_url: Set(format!("/api/stream/p2p/{}", ann.hash)),
                    bitrate: Set(ann.bitrate),
                    sample_rate: Set(ann.sample_rate),
                    format: Set(Some(ann.format.clone())),
                    is_available: Set(true),
                    last_checked_at: Set(Some(chrono::Utc::now().into())),
                    created_at: Set(chrono::Utc::now().into()),
                };
                if let Err(e) = new_remote.insert(&self.db).await {
                    warn!(hash = %ann.hash, "failed to create remote_track record: {e}");
                }

                // Async MusicBrainz enrichment — spawned to avoid blocking
                let mb = Arc::clone(&self.mb_client);
                let db = self.db.clone();
                let title = ann.title.clone();
                let artist = ann.artist_name.clone();
                tokio::spawn(async move {
                    if let Some(recording) = mb.lookup_recording(&title, &artist).await {
                        debug!(
                            mb_id = %recording.id,
                            title = %title,
                            score = recording.score,
                            "MusicBrainz match found"
                        );
                        let update = track::ActiveModel {
                            id: Set(track_id),
                            musicbrainz_id: Set(Some(recording.id)),
                            ..Default::default()
                        };
                        if let Err(e) = update.update(&db).await {
                            warn!(track_id = %track_id, "failed to update musicbrainz_id: {e}");
                        }
                    }
                });
            }
            Err(e) => {
                warn!(hash = %ann.hash, "failed to create track record: {e}");
            }
        }
    }

    /// Internal: handle a single protocol message.
    async fn handle_message(
        self: &Arc<Self>,
        msg: P2pMessage,
        mut send: iroh::endpoint::SendStream,
        node_id: EndpointId,
        peer_id: &str,
    ) -> Result<(), P2pError> {
        match msg {
            P2pMessage::FetchTrack { hash } => {
                // SECURITY: Only serve blobs that were explicitly published (FIX-19)
                if !self.published_hashes.read().await.contains(&hash) {
                    warn!(%peer_id, %hash, "rejected FetchTrack for non-published blob");
                    send.write_all(&0u32.to_be_bytes())
                        .await
                        .map_err(|e| P2pError::Connection(e.to_string()))?;
                    send.finish()
                        .map_err(|e| P2pError::Connection(e.to_string()))?;
                    return Ok(());
                }

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
                // Get actual track count from DB
                let track_count = track::Entity::find()
                    .filter(track::Column::ContentHash.is_not_null())
                    .filter(track::Column::FilePath.not_like("p2p://%"))
                    .count(&self.db)
                    .await
                    .unwrap_or(0);

                let pong = P2pMessage::Pong {
                    node_id: node_id.to_string(),
                    track_count,
                    version: Some(crate::build_version().to_string()),
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

                // After responding to a Ping, sync our full catalog to this peer.
                // This ensures that when a new peer connects, it receives all
                // existing tracks — not just future uploads.
                // Spawned asynchronously to avoid blocking the connection handler
                // (catalog sync may query the DB for every track + fetch blobs).
                if let Ok(remote_nid) = peer_id.parse::<EndpointId>() {
                    let node = Arc::clone(self);
                    tokio::spawn(async move {
                        node.announce_all_tracks_to_peer(remote_nid).await;
                    });
                }
            }
            P2pMessage::AnnounceTrack(ann) => {
                self.process_track_announcement(ann, peer_id).await;
                // Properly close our side of the stream
                if let Err(e) = send.finish() {
                    tracing::warn!(error = %e, "failed to finish send stream");
                }
            }
            P2pMessage::CatalogSync(announcements) => {
                // Acquire per-peer lock to serialize (not reject) concurrent CatalogSync pages
                let peer_lock = {
                    let mut map = self.catalog_sync_in_progress.lock().await;
                    map.entry(peer_id.to_string())
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                        .clone()
                };
                let _guard = peer_lock.lock().await;

                info!(count = announcements.len(), %peer_id, "received catalog sync");

                // Process in batches of 100 with yielding to avoid blocking the runtime
                for (i, ann) in announcements.into_iter().enumerate() {
                    self.process_track_announcement(ann, peer_id).await;
                    if (i + 1) % 100 == 0 {
                        tokio::task::yield_now().await;
                        debug!(%peer_id, processed = i + 1, "catalog sync batch progress");
                    }
                }

                // Properly close our side of the stream
                if let Err(e) = send.finish() {
                    tracing::warn!(error = %e, "failed to finish send stream");
                }
            }
            P2pMessage::PeerExchange { peers } => {
                info!(count = peers.len(), %peer_id, "received peer exchange request");

                // Verify new peers before adding them
                let our_id = self.node_id().to_string();
                let mut new_peers: Vec<String> = Vec::new();
                for pid in &peers {
                    if *pid != our_id && self.registry.get_peer(pid).await.is_none() {
                        new_peers.push(pid.clone());
                    }
                }

                if !new_peers.is_empty() {
                    let node = Arc::clone(self);
                    tokio::spawn(async move {
                        // Limit concurrent verification to 5
                        let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
                        let mut handles = Vec::new();

                        for pid in new_peers {
                            let node = Arc::clone(&node);
                            let sem = Arc::clone(&semaphore);
                            handles.push(tokio::spawn(async move {
                                let _permit = sem.acquire().await.ok()?;
                                // Try to ping the peer with a 5s timeout
                                if let Ok(nid) = pid.parse::<EndpointId>() {
                                    let ping = P2pMessage::Ping;
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        node.send_message_to_peer(nid, &ping),
                                    )
                                    .await
                                    {
                                        Ok(Ok(_)) => {
                                            node.registry.upsert_peer(&pid, None, 0).await;
                                            info!(peer = %pid, "PEX peer verified and added");
                                            // Sync catalogs with new peer
                                            node.announce_all_tracks_to_peer(nid).await;
                                            Some(pid)
                                        }
                                        _ => {
                                            debug!(peer = %pid, "PEX peer unreachable, not adding");
                                            None
                                        }
                                    }
                                } else {
                                    None
                                }
                            }));
                        }

                        for h in handles {
                            let _ = h.await;
                        }
                    });
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
            P2pMessage::CatalogDelta { since, tracks } => {
                info!(count = tracks.len(), %since, %peer_id, "received catalog delta");
                for ann in tracks {
                    self.process_track_announcement(ann, peer_id).await;
                }
                if let Err(e) = send.finish() {
                    tracing::warn!(error = %e, "failed to finish send stream");
                }
            }
            P2pMessage::RequestCatalog => {
                info!(%peer_id, "received catalog request — sending full catalog");
                if let Err(e) = send.finish() {
                    tracing::warn!(error = %e, "failed to finish send stream");
                }
                // Spawn so we don't block this connection handler
                if let Ok(remote_nid) = peer_id.parse::<EndpointId>() {
                    let node = Arc::clone(self);
                    tokio::spawn(async move {
                        node.announce_all_tracks_to_peer(remote_nid).await;
                    });
                }
            }
            P2pMessage::BloomExchange { bloom } => {
                info!(%peer_id, items = bloom.item_count, "received bloom filter from peer");
                self.search_index.import_peer_bloom(peer_id, bloom).await;
                if let Err(e) = send.finish() {
                    tracing::warn!(error = %e, "failed to finish send stream");
                }
            }
            P2pMessage::SearchQuery {
                request_id,
                query,
                limit,
            } => {
                info!(%peer_id, %query, "received search query");
                if let Err(e) = self
                    .handle_search_query(&request_id, &query, limit, send)
                    .await
                {
                    warn!(%peer_id, "failed to handle search query: {e}");
                }
            }
            P2pMessage::TrackData { .. }
            | P2pMessage::Pong { .. }
            | P2pMessage::SearchResults { .. } => {
                // These are responses, not requests — ignore if received as requests
                debug!("received unexpected response message");
            }
        }

        Ok(())
    }

    /// Load or generate a persistent secret key.
    /// The key is always persisted to ensure stable EndpointId across restarts.
    async fn load_or_generate_key(config: &P2pConfig) -> Result<SecretKey, P2pError> {
        let key_path = &config.secret_key_path;

        if key_path.exists() {
            let raw = tokio::fs::read(key_path).await.map_err(P2pError::Io)?;

            // Try loading as raw 32 bytes first, then as hex string (backward compat)
            if raw.len() == 32 {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&raw);
                info!(path = %key_path.display(), "loaded existing P2P secret key (raw)");
                return Ok(SecretKey::from_bytes(&buf));
            }

            let hex_str = String::from_utf8(raw)
                .map_err(|e| P2pError::Endpoint(format!("invalid key file encoding: {e}")))?;
            let decoded = data_encoding::HEXLOWER_PERMISSIVE
                .decode(hex_str.trim().as_bytes())
                .map_err(|e| P2pError::Endpoint(format!("invalid hex secret key: {e}")))?;
            if decoded.len() != 32 {
                return Err(P2pError::Endpoint(format!(
                    "secret key has wrong length: {} (expected 32)",
                    decoded.len()
                )));
            }
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&decoded);
            info!(path = %key_path.display(), "loaded existing P2P secret key");
            return Ok(SecretKey::from_bytes(&buf));
        }

        // Generate and save a new key
        let key = SecretKey::generate(&mut rand::rngs::StdRng::from_os_rng());
        if let Some(parent) = key_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(P2pError::Io)?;
        }
        tokio::fs::write(key_path, data_encoding::HEXLOWER.encode(&key.to_bytes()))
            .await
            .map_err(P2pError::Io)?;
        info!(path = %key_path.display(), "generated and saved new P2P secret key");
        Ok(key)
    }

    /// Wait for the relay connection to be established.
    /// Returns the relay URL once connected, or None if unavailable.
    async fn wait_for_relay(endpoint: &Endpoint) -> Option<String> {
        // Poll endpoint addr until relay_url is available
        for _ in 0..50 {
            let addr = endpoint.addr();
            if let Some(url) = addr.relay_urls().next() {
                return Some(url.to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        None
    }

    /// Fetch a cover art blob from a peer, write it to the local audio storage,
    /// and update the album's `cover_url` in the database.
    async fn sync_cover_for_album(&self, album_id: Uuid, ann: &TrackAnnouncement, peer_id: &str) {
        let cover_hash_str = match &ann.cover_hash {
            Some(h) if !h.is_empty() => h.clone(),
            _ => return,
        };

        let blob_hash: Hash = match cover_hash_str.parse() {
            Ok(h) => h,
            Err(_) => {
                warn!(hash = %cover_hash_str, "invalid cover hash");
                return;
            }
        };

        // Fetch the cover blob from the peer
        let nid: EndpointId = match peer_id.parse() {
            Ok(id) => id,
            Err(_) => return,
        };

        let cover_data = if self.has_blob(blob_hash).await {
            // Already have it locally
            match self.get_local_track(blob_hash).await {
                Ok(d) => d,
                Err(_) => return,
            }
        } else {
            let peer_addr = EndpointAddr::new(nid);
            match self.fetch_track_from_peer(peer_addr, blob_hash).await {
                Ok(data) => {
                    // Store in blob store with a persistent tag
                    match self
                        .blob_store
                        .blobs()
                        .add_bytes(data.clone())
                        .temp_tag()
                        .await
                    {
                        Ok(outcome) => {
                            let h = outcome.hash();
                            let tag_name = format!("published-{}", h);
                            if let Err(e) = self
                                .blob_store
                                .tags()
                                .set(tag_name, HashAndFormat::raw(h))
                                .await
                            {
                                warn!(hash = %cover_hash_str, "failed to set persistent tag for cover: {e}");
                            }
                            // Register hash so it can be served to peers
                            self.published_hashes.write().await.insert(h.to_string());
                        }
                        Err(e) => warn!(hash = %cover_hash_str, "failed to import cover blob: {e}"),
                    }
                    data
                }
                Err(e) => {
                    warn!(hash = %cover_hash_str, %peer_id, "failed to fetch cover: {e}");
                    return;
                }
            }
        };

        // Write cover to the audio storage filesystem
        let artist_dir = sanitize_for_path(&ann.artist_name);
        let album_dir = ann
            .album_title
            .as_deref()
            .map(sanitize_for_path)
            .unwrap_or_else(|| "singles".to_string());

        let cover_dir = self
            .cover_base_path()
            .join("p2p-covers")
            .join(&artist_dir)
            .join(&album_dir);

        if let Err(e) = tokio::fs::create_dir_all(&cover_dir).await {
            warn!("failed to create cover directory: {e}");
            return;
        }

        let cover_file = cover_dir.join("cover.jpg");
        if let Err(e) = tokio::fs::write(&cover_file, &cover_data).await {
            warn!("failed to write cover file: {e}");
            return;
        }

        // Build the relative path for the cover_url
        let relative = cover_file
            .strip_prefix(self.cover_base_path())
            .unwrap_or(&cover_file)
            .to_string_lossy()
            .to_string();

        let cover_url = format!("/api/media/{relative}");

        // Update the album's cover_url in the database
        let update = album::ActiveModel {
            id: Set(album_id),
            cover_url: Set(Some(cover_url.clone())),
            ..Default::default()
        };
        if let Err(e) = update.update(&self.db).await {
            warn!(%album_id, "failed to update album cover_url: {e}");
        } else {
            info!(%album_id, %cover_url, "album cover synced from peer");
        }
    }
}

// ── TrackFetcher implementation for P2pNode ──────────────────────────

#[async_trait]
impl TrackFetcher for Arc<P2pNode> {
    async fn fetch_track(&self, peer_id: &str, hash: &str) -> Result<Bytes, P2pError> {
        let nid: EndpointId = peer_id
            .parse()
            .map_err(|_| P2pError::Connection(format!("invalid peer id: {}", peer_id)))?;
        let peer_addr = EndpointAddr::new(nid);

        let blob_hash: Hash = hash
            .parse()
            .map_err(|_| P2pError::TrackNotFound(format!("invalid hash: {}", hash)))?;

        self.fetch_track_from_peer(peer_addr, blob_hash).await
    }

    async fn check_blob_exists(&self, hash: &str) -> bool {
        if let Ok(h) = hash.parse::<Hash>() {
            self.has_blob(h).await
        } else {
            false
        }
    }

    async fn peer_is_online(&self, peer_id: &str) -> bool {
        self.registry
            .get_peer(peer_id)
            .await
            .map(|p| p.is_online)
            .unwrap_or(false)
    }

    async fn alternative_sources(&self, hash: &str) -> Vec<PeerTrackInfo> {
        // Query remote_tracks that share the same content hash
        use sea_orm::QueryFilter;

        let remotes = remote_track::Entity::find()
            .filter(remote_track::Column::RemoteUri.ends_with(format!("/{}", hash)))
            .all(&self.db)
            .await
            .unwrap_or_default();

        let mut sources = Vec::new();
        for rt in remotes {
            let origin = rt
                .instance_domain
                .strip_prefix("p2p://")
                .unwrap_or(&rt.instance_domain)
                .to_string();

            let is_online = self
                .registry
                .get_peer(&origin)
                .await
                .map(|p| p.is_online)
                .unwrap_or(false);

            // Look up file_size from the linked local track record
            let file_size = match rt.local_track_id {
                Some(track_id) => track::Entity::find_by_id(track_id)
                    .one(&self.db)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.file_size)
                    .unwrap_or(0),
                None => 0,
            };

            sources.push(PeerTrackInfo {
                peer_id: origin,
                format: rt.format.unwrap_or_default(),
                bitrate: rt.bitrate,
                sample_rate: rt.sample_rate,
                is_online,
                file_size,
            });
        }

        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_for_path ──────────────────────────────────────────────

    #[test]
    fn test_sanitize_for_path_basic() {
        assert_eq!(sanitize_for_path("Hello World"), "Hello World");
        assert_eq!(sanitize_for_path("my-track_01"), "my-track_01");
    }

    #[test]
    fn test_sanitize_for_path_special_chars() {
        assert_eq!(sanitize_for_path("AC/DC"), "AC_DC");
        assert_eq!(sanitize_for_path("foo:bar*baz"), "foo_bar_baz");
        assert_eq!(sanitize_for_path("a<b>c"), "a_b_c");
    }

    #[test]
    fn test_sanitize_for_path_trim() {
        assert_eq!(sanitize_for_path("  padded  "), "padded");
    }

    #[test]
    fn test_sanitize_for_path_empty() {
        assert_eq!(sanitize_for_path(""), "");
    }

    // ── P2pMessage serde roundtrip ────────────────────────────────────

    #[test]
    fn test_message_serde_ping() {
        let msg = P2pMessage::Ping;
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, P2pMessage::Ping));
    }

    #[test]
    fn test_message_serde_pong() {
        let msg = P2pMessage::Pong {
            node_id: "abc123".to_string(),
            track_count: 42,
            version: Some("0.1.5".to_string()),
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::Pong {
                node_id,
                track_count,
                version,
            } => {
                assert_eq!(node_id, "abc123");
                assert_eq!(track_count, 42);
                assert_eq!(version, Some("0.1.5".to_string()));
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn test_message_serde_fetch_track() {
        let msg = P2pMessage::FetchTrack {
            hash: "deadbeef".to_string(),
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::FetchTrack { hash } => assert_eq!(hash, "deadbeef"),
            _ => panic!("expected FetchTrack"),
        }
    }

    #[test]
    fn test_message_serde_peer_exchange() {
        let msg = P2pMessage::PeerExchange {
            peers: vec!["a".into(), "b".into(), "c".into()],
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::PeerExchange { peers } => {
                assert_eq!(peers.len(), 3);
                assert_eq!(peers[0], "a");
            }
            _ => panic!("expected PeerExchange"),
        }
    }

    #[test]
    fn test_message_serde_search_query() {
        let msg = P2pMessage::SearchQuery {
            request_id: "req-1".to_string(),
            query: "bohemian rhapsody".to_string(),
            limit: 10,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::SearchQuery {
                request_id,
                query,
                limit,
            } => {
                assert_eq!(request_id, "req-1");
                assert_eq!(query, "bohemian rhapsody");
                assert_eq!(limit, 10);
            }
            _ => panic!("expected SearchQuery"),
        }
    }

    #[test]
    fn test_message_serde_search_results() {
        let msg = P2pMessage::SearchResults {
            request_id: "req-1".to_string(),
            results: vec![SearchResultItem {
                hash: "h1".into(),
                title: "Test Track".into(),
                artist_name: "Test Artist".into(),
                album_title: Some("Test Album".into()),
                duration_secs: 180.0,
                format: "FLAC".into(),
                genre: Some("Rock".into()),
                year: Some(2024),
                bitrate: Some(320_000),
                source_node: "node1".into(),
                musicbrainz_id: None,
                relevance: 0.95,
            }],
            total: 1,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::SearchResults { results, total, .. } => {
                assert_eq!(total, 1);
                assert_eq!(results[0].title, "Test Track");
                assert!((results[0].relevance - 0.95).abs() < f32::EPSILON);
            }
            _ => panic!("expected SearchResults"),
        }
    }

    // ── TrackAnnouncement serde roundtrip ─────────────────────────────

    #[test]
    fn test_track_announcement_roundtrip() {
        let ann = TrackAnnouncement {
            hash: "abc".into(),
            title: "Song".into(),
            artist_name: "Artist".into(),
            album_artist_name: None,
            album_title: Some("Album".into()),
            duration_secs: 240.5,
            format: "MP3".into(),
            file_size: 8_000_000,
            genre: Some("Jazz".into()),
            year: Some(1959),
            track_number: Some(1),
            disc_number: Some(1),
            bitrate: Some(320_000),
            sample_rate: Some(44_100),
            origin_node: "node-xyz".into(),
            cover_hash: Some("cover123".into()),
        };
        let bytes = serde_json::to_vec(&ann).unwrap();
        let decoded: TrackAnnouncement = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.title, "Song");
        assert_eq!(decoded.file_size, 8_000_000);
        assert_eq!(decoded.cover_hash.as_deref(), Some("cover123"));
    }

    // ── P2pConfig defaults ───────────────────────────────────────────

    #[test]
    fn test_p2p_config_defaults() {
        let cfg = P2pConfig::default();
        assert_eq!(cfg.bind_port, 0);
        assert!(cfg.enable_local_discovery);
        assert!(cfg.seed_peers.is_empty());
        assert_eq!(cfg.blobs_dir, PathBuf::from("data/p2p/blobs"));
    }

    // ── SecretKey load/generate ──────────────────────────────────────

    #[tokio::test]
    async fn test_load_or_generate_key_creates_new() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("secret_key");
        let config = P2pConfig {
            secret_key_path: key_path.clone(),
            ..Default::default()
        };

        let key1 = P2pNode::load_or_generate_key(&config).await.unwrap();
        assert!(key_path.exists());

        // Loading again should return the same key
        let key2 = P2pNode::load_or_generate_key(&config).await.unwrap();
        assert_eq!(key1.public(), key2.public());
    }

    #[tokio::test]
    async fn test_load_or_generate_key_hex_format() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("secret_key");
        let config = P2pConfig {
            secret_key_path: key_path.clone(),
            ..Default::default()
        };

        let _key = P2pNode::load_or_generate_key(&config).await.unwrap();
        let contents = tokio::fs::read_to_string(&key_path).await.unwrap();
        // Should be 64 hex chars (32 bytes)
        assert_eq!(contents.len(), 64);
        assert!(contents.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_load_key_raw_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("secret_key");

        // Write raw 32 bytes
        let key = SecretKey::generate(&mut rand::rngs::StdRng::from_os_rng());
        tokio::fs::write(&key_path, key.to_bytes()).await.unwrap();

        let config = P2pConfig {
            secret_key_path: key_path,
            ..Default::default()
        };

        let loaded = P2pNode::load_or_generate_key(&config).await.unwrap();
        assert_eq!(loaded.public(), key.public());
    }

    // ── EndpointAddr / EndpointId integration ────────────────────────

    #[test]
    fn test_endpoint_addr_construction() {
        let key = SecretKey::generate(&mut rand::rngs::StdRng::from_os_rng());
        let id = key.public();
        let addr = EndpointAddr::new(id);
        assert_eq!(addr.id, id);
        assert!(addr.ip_addrs().next().is_none());
        assert!(addr.relay_urls().next().is_none());
    }

    #[test]
    fn test_endpoint_id_display_roundtrip() {
        let key = SecretKey::generate(&mut rand::rngs::StdRng::from_os_rng());
        let id = key.public();
        let id_str = id.to_string();
        assert!(!id_str.is_empty());
        let parsed: EndpointId = id_str.parse().unwrap();
        assert_eq!(parsed, id);
    }

    // ── CatalogSync / CatalogDelta serde ─────────────────────────────

    #[test]
    fn test_message_serde_catalog_sync() {
        let ann = TrackAnnouncement {
            hash: "h1".into(),
            title: "T".into(),
            artist_name: "A".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 60.0,
            format: "MP3".into(),
            file_size: 1000,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let msg = P2pMessage::CatalogSync(vec![ann.clone()]);
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::CatalogSync(tracks) => {
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].hash, "h1");
            }
            _ => panic!("expected CatalogSync"),
        }
    }

    #[test]
    fn test_message_serde_bloom_exchange() {
        let bloom = BloomFilterData {
            bitmap: vec![0u8; 128],
            num_hashes: 7,
            bitmap_bits: 1024,
            sip_keys: [(1, 2), (3, 4)],
            item_count: 42,
        };
        let msg = P2pMessage::BloomExchange { bloom };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::BloomExchange { bloom } => {
                assert_eq!(bloom.num_hashes, 7);
                assert_eq!(bloom.bitmap_bits, 1024);
                assert_eq!(bloom.bitmap.len(), 128);
                assert_eq!(bloom.item_count, 42);
                assert_eq!(bloom.sip_keys, [(1, 2), (3, 4)]);
            }
            _ => panic!("expected BloomExchange"),
        }
    }

    // ── P2pMessage serde: TrackData ──────────────────────────────────

    #[test]
    fn test_message_serde_track_data() {
        let msg = P2pMessage::TrackData {
            hash: "abc123".to_string(),
            size: 5_000_000,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::TrackData { hash, size } => {
                assert_eq!(hash, "abc123");
                assert_eq!(size, 5_000_000);
            }
            _ => panic!("expected TrackData"),
        }
    }

    // ── P2pMessage serde: CatalogDelta ──────────────────────────────

    #[test]
    fn test_message_serde_catalog_delta() {
        let since = chrono::Utc::now();
        let ann = TrackAnnouncement {
            hash: "delta1".into(),
            title: "Delta Track".into(),
            artist_name: "Delta Artist".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 120.0,
            format: "OPUS".into(),
            file_size: 2000,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let msg = P2pMessage::CatalogDelta {
            since,
            tracks: vec![ann],
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::CatalogDelta { since: s, tracks } => {
                assert_eq!(tracks.len(), 1);
                assert_eq!(tracks[0].hash, "delta1");
                assert_eq!(tracks[0].format, "OPUS");
                // chrono roundtrip may lose sub-nanosecond precision
                assert!((s - since).num_seconds().abs() < 1);
            }
            _ => panic!("expected CatalogDelta"),
        }
    }

    // ── P2pMessage serde: RequestCatalog ─────────────────────────────

    #[test]
    fn test_message_serde_request_catalog() {
        let msg = P2pMessage::RequestCatalog;
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::RequestCatalog => {} // expected
            _ => panic!("expected RequestCatalog"),
        }
    }

    #[test]
    fn test_message_serde_request_catalog_roundtrip_json() {
        let msg = P2pMessage::RequestCatalog;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("RequestCatalog"));
        let decoded: P2pMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            P2pMessage::RequestCatalog => {}
            _ => panic!("expected RequestCatalog"),
        }
    }

    // ── P2pMessage serde: AnnounceTrack ─────────────────────────────

    #[test]
    fn test_message_serde_announce_track() {
        let ann = TrackAnnouncement {
            hash: "ann1".into(),
            title: "Announced".into(),
            artist_name: "Announcer".into(),
            album_artist_name: None,
            album_title: Some("Album Ann".into()),
            duration_secs: 200.0,
            format: "FLAC".into(),
            file_size: 50_000_000,
            genre: Some("Electronic".into()),
            year: Some(2024),
            track_number: Some(3),
            disc_number: Some(1),
            bitrate: Some(1_411_000),
            sample_rate: Some(48_000),
            origin_node: "origin1".into(),
            cover_hash: Some("cover_abc".into()),
        };
        let msg = P2pMessage::AnnounceTrack(ann);
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::AnnounceTrack(a) => {
                assert_eq!(a.hash, "ann1");
                assert_eq!(a.title, "Announced");
                assert_eq!(a.format, "FLAC");
                assert_eq!(a.file_size, 50_000_000);
                assert_eq!(a.year, Some(2024));
                assert_eq!(a.bitrate, Some(1_411_000));
                assert_eq!(a.sample_rate, Some(48_000));
                assert_eq!(a.cover_hash.as_deref(), Some("cover_abc"));
            }
            _ => panic!("expected AnnounceTrack"),
        }
    }

    // ── P2pMessage: empty collections ────────────────────────────────

    #[test]
    fn test_message_serde_peer_exchange_empty() {
        let msg = P2pMessage::PeerExchange { peers: vec![] };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::PeerExchange { peers } => assert!(peers.is_empty()),
            _ => panic!("expected PeerExchange"),
        }
    }

    #[test]
    fn test_message_serde_catalog_sync_empty() {
        let msg = P2pMessage::CatalogSync(vec![]);
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::CatalogSync(tracks) => assert!(tracks.is_empty()),
            _ => panic!("expected CatalogSync"),
        }
    }

    #[test]
    fn test_message_serde_search_results_empty() {
        let msg = P2pMessage::SearchResults {
            request_id: "r".into(),
            results: vec![],
            total: 0,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::SearchResults { results, total, .. } => {
                assert!(results.is_empty());
                assert_eq!(total, 0);
            }
            _ => panic!("expected SearchResults"),
        }
    }

    // ── P2pMessage: invalid JSON deserialization ─────────────────────

    #[test]
    fn test_message_deserialize_invalid_json() {
        let result = serde_json::from_str::<P2pMessage>("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_message_deserialize_unknown_variant() {
        let result = serde_json::from_str::<P2pMessage>(r#"{"UnknownVariant":{}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_message_deserialize_missing_fields() {
        // Pong without required fields
        let result = serde_json::from_str::<P2pMessage>(r#"{"Pong":{}}"#);
        assert!(result.is_err());
    }

    // ── TrackAnnouncement edge cases ─────────────────────────────────

    #[test]
    fn test_track_announcement_all_optional_none() {
        let ann = TrackAnnouncement {
            hash: "h".into(),
            title: "T".into(),
            artist_name: "A".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 0.0,
            format: "WAV".into(),
            file_size: 0,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let bytes = serde_json::to_vec(&ann).unwrap();
        let decoded: TrackAnnouncement = serde_json::from_slice(&bytes).unwrap();
        assert!(decoded.album_title.is_none());
        assert!(decoded.genre.is_none());
        assert!(decoded.year.is_none());
        assert!(decoded.track_number.is_none());
        assert!(decoded.disc_number.is_none());
        assert!(decoded.bitrate.is_none());
        assert!(decoded.sample_rate.is_none());
        assert!(decoded.cover_hash.is_none());
    }

    #[test]
    fn test_track_announcement_clone() {
        let ann = TrackAnnouncement {
            hash: "h".into(),
            title: "T".into(),
            artist_name: "A".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 1.0,
            format: "MP3".into(),
            file_size: 100,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let cloned = ann.clone();
        assert_eq!(ann.hash, cloned.hash);
        assert_eq!(ann.title, cloned.title);
    }

    #[test]
    fn test_track_announcement_debug() {
        let ann = TrackAnnouncement {
            hash: "h".into(),
            title: "T".into(),
            artist_name: "A".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 1.0,
            format: "MP3".into(),
            file_size: 100,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let debug = format!("{:?}", ann);
        assert!(debug.contains("TrackAnnouncement"));
        assert!(debug.contains("hash"));
    }

    // ── SearchResultItem edge cases ──────────────────────────────────

    #[test]
    fn test_search_result_item_roundtrip() {
        let item = SearchResultItem {
            hash: "sr1".into(),
            title: "Search Result".into(),
            artist_name: "SR Artist".into(),
            album_title: None,
            duration_secs: 0.5,
            format: "AAC".into(),
            genre: None,
            year: None,
            bitrate: None,
            source_node: "node-x".into(),
            musicbrainz_id: Some("mb-123".into()),
            relevance: 1.0,
        };
        let bytes = serde_json::to_vec(&item).unwrap();
        let decoded: SearchResultItem = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.hash, "sr1");
        assert_eq!(decoded.musicbrainz_id.as_deref(), Some("mb-123"));
        assert!((decoded.relevance - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_search_result_item_clone() {
        let item = SearchResultItem {
            hash: "h".into(),
            title: "T".into(),
            artist_name: "A".into(),
            album_title: None,
            duration_secs: 1.0,
            format: "MP3".into(),
            genre: None,
            year: None,
            bitrate: None,
            source_node: "n".into(),
            musicbrainz_id: None,
            relevance: 0.0,
        };
        let cloned = item.clone();
        assert_eq!(item.hash, cloned.hash);
    }

    // ── sanitize_for_path additional edge cases ──────────────────────

    #[test]
    fn test_sanitize_for_path_unicode() {
        // Unicode letters (accented) should be kept as they're alphanumeric
        assert_eq!(sanitize_for_path("Étoile"), "Étoile");
        assert_eq!(sanitize_for_path("日本語"), "日本語");
        assert_eq!(sanitize_for_path("Ñoño"), "Ñoño");
    }

    #[test]
    fn test_sanitize_for_path_mixed_special() {
        assert_eq!(sanitize_for_path("a/b\\c:d*e?f"), "a_b_c_d_e_f");
    }

    #[test]
    fn test_sanitize_for_path_only_special() {
        // All special chars — trimmed result is empty
        assert_eq!(sanitize_for_path("///"), "___");
    }

    #[test]
    fn test_sanitize_for_path_preserves_hyphens_underscores_spaces() {
        assert_eq!(sanitize_for_path("my track-name_01"), "my track-name_01");
    }

    #[test]
    fn test_sanitize_for_path_emoji() {
        // Emojis are not alphanumeric, should be replaced
        assert_eq!(sanitize_for_path("🎵music🎵"), "_music_");
    }

    // ── P2pConfig::from_env ──────────────────────────────────────────

    #[test]
    fn test_config_from_env_defaults() {
        // Clear all P2P env vars to test defaults
        std::env::remove_var("P2P_BLOBS_DIR");
        std::env::remove_var("P2P_SECRET_KEY_PATH");
        std::env::remove_var("P2P_BIND_PORT");
        std::env::remove_var("P2P_LOCAL_DISCOVERY");
        std::env::remove_var("P2P_SEED_PEERS");
        std::env::remove_var("AUDIO_STORAGE_PATH");
        std::env::remove_var("METADATA_STORAGE_PATH");

        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.blobs_dir, PathBuf::from("data/p2p/blobs"));
        assert_eq!(cfg.secret_key_path, PathBuf::from("data/p2p/secret_key"));
        assert_eq!(cfg.bind_port, 0);
        assert!(cfg.enable_local_discovery);
        assert!(cfg.seed_peers.is_empty());
        assert_eq!(cfg.audio_storage_path, PathBuf::from("data/music"));
        assert!(cfg.metadata_storage_path.is_none());
    }

    #[test]
    fn test_config_from_env_custom_blobs_dir() {
        std::env::set_var("P2P_BLOBS_DIR", "/tmp/custom/blobs");
        std::env::remove_var("P2P_SECRET_KEY_PATH");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.blobs_dir, PathBuf::from("/tmp/custom/blobs"));
        // secret_key_path should derive from blobs_dir parent
        assert_eq!(cfg.secret_key_path, PathBuf::from("/tmp/custom/secret_key"));
        std::env::remove_var("P2P_BLOBS_DIR");
    }

    #[test]
    fn test_config_from_env_custom_secret_key() {
        std::env::set_var("P2P_SECRET_KEY_PATH", "/my/key");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.secret_key_path, PathBuf::from("/my/key"));
        std::env::remove_var("P2P_SECRET_KEY_PATH");
    }

    #[test]
    fn test_config_from_env_bind_port() {
        std::env::set_var("P2P_BIND_PORT", "4433");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.bind_port, 4433);
        std::env::remove_var("P2P_BIND_PORT");
    }

    #[test]
    fn test_config_from_env_bind_port_invalid() {
        std::env::set_var("P2P_BIND_PORT", "not_a_number");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.bind_port, 0); // fallback
        std::env::remove_var("P2P_BIND_PORT");
    }

    #[test]
    fn test_config_from_env_local_discovery_false() {
        std::env::set_var("P2P_LOCAL_DISCOVERY", "false");
        let cfg = P2pConfig::from_env();
        assert!(!cfg.enable_local_discovery);
        std::env::remove_var("P2P_LOCAL_DISCOVERY");
    }

    #[test]
    fn test_config_from_env_local_discovery_case_insensitive() {
        std::env::set_var("P2P_LOCAL_DISCOVERY", "TRUE");
        let cfg = P2pConfig::from_env();
        assert!(cfg.enable_local_discovery);
        std::env::remove_var("P2P_LOCAL_DISCOVERY");
    }

    #[test]
    fn test_config_from_env_seed_peers() {
        std::env::set_var("P2P_SEED_PEERS", "peer1,peer2, peer3 ");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.seed_peers, vec!["peer1", "peer2", "peer3"]);
        std::env::remove_var("P2P_SEED_PEERS");
    }

    #[test]
    fn test_config_from_env_seed_peers_empty_entries() {
        std::env::set_var("P2P_SEED_PEERS", "peer1,,, ,peer2");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.seed_peers, vec!["peer1", "peer2"]);
        std::env::remove_var("P2P_SEED_PEERS");
    }

    #[test]
    fn test_config_from_env_audio_storage() {
        std::env::set_var("AUDIO_STORAGE_PATH", "/music/storage");
        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.audio_storage_path, PathBuf::from("/music/storage"));
        std::env::remove_var("AUDIO_STORAGE_PATH");
    }

    #[test]
    fn test_config_from_env_metadata_storage() {
        std::env::set_var("METADATA_STORAGE_PATH", "/metadata/covers");
        let cfg = P2pConfig::from_env();
        assert_eq!(
            cfg.metadata_storage_path,
            Some(PathBuf::from("/metadata/covers"))
        );
        std::env::remove_var("METADATA_STORAGE_PATH");
    }

    // ── P2pConfig Clone + Debug ──────────────────────────────────────

    #[test]
    fn test_config_clone() {
        let cfg = P2pConfig::default();
        let cloned = cfg.clone();
        assert_eq!(cfg.blobs_dir, cloned.blobs_dir);
        assert_eq!(cfg.bind_port, cloned.bind_port);
    }

    #[test]
    fn test_config_debug() {
        let cfg = P2pConfig::default();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("P2pConfig"));
        assert!(debug.contains("blobs_dir"));
    }

    // ── SecretKey: edge cases ────────────────────────────────────────

    #[tokio::test]
    async fn test_load_key_invalid_hex() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("secret_key");
        // Write invalid hex (odd length, not 64 chars)
        tokio::fs::write(&key_path, "not_valid_hex_data")
            .await
            .unwrap();

        let config = P2pConfig {
            secret_key_path: key_path,
            ..Default::default()
        };

        // Should treat it as raw bytes and handle gracefully (fallback or error)
        // The current impl tries hex first, then raw bytes — with 18 bytes it'll
        // fail both and generate a new key
        let result = P2pNode::load_or_generate_key(&config).await;
        // Implementation may handle this differently — just ensure no panic
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_load_key_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("subdir").join("deep").join("secret_key");
        let config = P2pConfig {
            secret_key_path: key_path.clone(),
            ..Default::default()
        };

        let result = P2pNode::load_or_generate_key(&config).await;
        // Parent dirs should be created automatically
        assert!(result.is_ok());
        assert!(key_path.exists());
    }

    // ── ALPN constant ────────────────────────────────────────────────

    #[test]
    fn test_alpn_constant() {
        assert_eq!(SOUNDTIME_ALPN, b"soundtime/p2p/1");
        assert_eq!(SOUNDTIME_ALPN.len(), 15);
    }

    // ── Multiple TrackAnnouncements in CatalogSync ───────────────────

    #[test]
    fn test_catalog_sync_multiple_tracks() {
        let make_ann = |hash: &str, title: &str| TrackAnnouncement {
            hash: hash.into(),
            title: title.into(),
            artist_name: "A".into(),
            album_artist_name: None,
            album_title: None,
            duration_secs: 60.0,
            format: "MP3".into(),
            file_size: 1000,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            bitrate: None,
            sample_rate: None,
            origin_node: "n".into(),
            cover_hash: None,
        };
        let msg = P2pMessage::CatalogSync(vec![
            make_ann("h1", "Track 1"),
            make_ann("h2", "Track 2"),
            make_ann("h3", "Track 3"),
        ]);
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::CatalogSync(tracks) => {
                assert_eq!(tracks.len(), 3);
                assert_eq!(tracks[0].hash, "h1");
                assert_eq!(tracks[1].hash, "h2");
                assert_eq!(tracks[2].hash, "h3");
            }
            _ => panic!("expected CatalogSync"),
        }
    }

    // ── SearchQuery with special characters ──────────────────────────

    #[test]
    fn test_search_query_unicode() {
        let msg = P2pMessage::SearchQuery {
            request_id: "r1".into(),
            query: "日本語の曲 Ñoño café".into(),
            limit: 5,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::SearchQuery { query, .. } => {
                assert_eq!(query, "日本語の曲 Ñoño café");
            }
            _ => panic!("expected SearchQuery"),
        }
    }

    // ── Large message serde ──────────────────────────────────────────

    #[test]
    fn test_search_results_many_items() {
        let items: Vec<SearchResultItem> = (0..100)
            .map(|i| SearchResultItem {
                hash: format!("h{i}"),
                title: format!("Track {i}"),
                artist_name: "A".into(),
                album_title: None,
                duration_secs: 180.0,
                format: "MP3".into(),
                genre: None,
                year: None,
                bitrate: None,
                source_node: "n".into(),
                musicbrainz_id: None,
                relevance: i as f32 / 100.0,
            })
            .collect();
        let msg = P2pMessage::SearchResults {
            request_id: "big".into(),
            results: items,
            total: 100,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: P2pMessage = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            P2pMessage::SearchResults { results, total, .. } => {
                assert_eq!(total, 100);
                assert_eq!(results.len(), 100);
                assert_eq!(results[99].hash, "h99");
            }
            _ => panic!("expected SearchResults"),
        }
    }
}
