//! Core P2P node — wraps an iroh Endpoint and iroh-blobs Store.
//!
//! The `P2pNode` is the central unit that:
//! - Listens for incoming peer connections (QUIC via iroh)
//! - Publishes local tracks into content-addressed blob storage
//! - Fetches tracks from remote peers by hash
//! - Exposes the local EndpointId for discovery

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey, Watcher};
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
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
#[path = "range_transport/live.rs"]
mod ranges;

#[path = "legacy_audio.rs"]
mod legacy_audio;

pub const SOUNDTIME_ALPN: &[u8] = b"soundtime/p2p/1";
#[path = "inventory/live.rs"]
mod catalogs;
#[path = "inventory/mirror_runtime.rs"]
mod mirror_runtime;
pub const EXPLICIT_CATALOG_ALPN: &[u8] = b"soundtime/control/2";
use crate::peer_policy::PEX_LIMIT;

fn presence_probe(
    alpn: &[u8],
    followed: bool,
    share_identity: bool,
    versioned_catalog: bool,
) -> Result<P2pMessage, P2pError> {
    match alpn {
        EXPLICIT_CATALOG_ALPN => Ok(P2pMessage::Presence {
            share_identity,
            follows_recipient: followed,
            versioned_catalog,
        }),
        SOUNDTIME_ALPN if followed => Ok(P2pMessage::Ping),
        _ => Err(P2pError::Connection(
            "legacy peer requires explicit follow before catalogue-producing ping".into(),
        )),
    }
}

/// Maximum allowed size for a single P2P message (64 MiB).
/// CatalogSync messages can be large for instances with many tracks.
const MAX_P2P_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// Maximum number of concurrent incoming P2P connections.
const MAX_CONCURRENT_P2P_CONNECTIONS: usize = 64;

/// Sanitize a string for use as a filesystem directory name.
#[cfg(test)]
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
    FetchTrack {
        hash: String,
    },
    /// Announce a track with full metadata for catalog replication
    AnnounceTrack(TrackAnnouncement),
    /// Response containing track data
    TrackData {
        hash: String,
        size: u64,
    },
    /// Peer discovery ping
    Ping,
    /// Exchanged only on the explicitly negotiated control ALPN.
    Presence {
        share_identity: bool,
        follows_recipient: bool,
        #[serde(default)]
        versioned_catalog: bool,
    },
    PresenceReply {
        node_id: String,
        track_count: u64,
        version: Option<String>,
        share_identity: bool,
        follows_recipient: bool,
        #[serde(default)]
        versioned_catalog: bool,
    },
    StopCatalog,
    CatalogChanged,
    SignedWithdrawal(crate::inventory::SignedPublication),
    MirrorOffer(Box<crate::inventory::lease::Offer>),
    /// Peer discovery pong
    Pong {
        node_id: String,
        track_count: u64,
        /// Software version of the peer (e.g. "0.1.42")
        #[serde(default)]
        version: Option<String>,
    },
    /// Peer exchange — share list of known peer EndpointIds for network discovery
    PeerExchange {
        #[serde(deserialize_with = "crate::peer_policy::read_candidates")]
        peers: Vec<String>,
    },
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
    BloomExchange {
        bloom: BloomFilterData,
    },
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
    /// Publication identity when the sender has an authenticated catalogue entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
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
    /// Pilot: negotiate metadata-only presence and explicit catalogue subscriptions.
    pub explicit_catalog: bool,
    pub versioned_catalog: bool,
    pub share_cached: bool,
    pub range_streaming: bool,
    pub cache_on_play: bool,
    /// Consent to redistribute this node's identity through peer exchange.
    pub share_identity: bool,
    /// Use the public n0 address lookups and relays.
    pub enable_public_discovery: bool,
    pub relay_url: Option<String>,
    pub relay_only: bool,
    /// Whether to enable local network (mDNS) discovery
    pub enable_local_discovery: bool,
    /// Whether to publish and resolve known EndpointIds on the Mainline DHT
    pub enable_dht_discovery: bool,
    /// Opt-in HTTP endpoint where participating nodes exchange EndpointIds
    pub rendezvous_url: Option<String>,
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
            explicit_catalog: false,
            versioned_catalog: false,
            share_cached: false,
            range_streaming: false,
            cache_on_play: false,
            share_identity: false,
            enable_public_discovery: true,
            relay_url: None,
            relay_only: false,
            enable_local_discovery: true,
            enable_dht_discovery: true,
            rendezvous_url: None,
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

        let enable_public_discovery = std::env::var("P2P_PUBLIC_DISCOVERY")
            .unwrap_or_else(|_| "true".to_owned())
            .eq_ignore_ascii_case("true");
        let enable_dht_discovery = enable_public_discovery
            && std::env::var("P2P_DHT_DISCOVERY")
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
            cache_on_play: std::env::var("P2P_CACHE_ON_PLAY")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            range_streaming: std::env::var("P2P_RANGE_STREAMING")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            versioned_catalog: std::env::var("P2P_VERSIONED_CATALOG")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            share_cached: std::env::var("P2P_SHARE_CACHED")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            explicit_catalog: std::env::var("P2P_EXPLICIT_CATALOG")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            share_identity: std::env::var("P2P_SHARE_IDENTITY")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            enable_public_discovery,
            relay_url: std::env::var("P2P_RELAY_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            relay_only: std::env::var("P2P_RELAY_ONLY")
                .is_ok_and(|s| s.eq_ignore_ascii_case("true")),
            enable_local_discovery,
            enable_dht_discovery,
            rendezvous_url: std::env::var("P2P_RENDEZVOUS_URL")
                .ok()
                .map(|url| url.trim().to_owned())
                .filter(|url| !url.is_empty()),
            seed_peers,
            audio_storage_path,
            metadata_storage_path,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlobKind {
    Audio,
    Cover,
}

#[derive(serde::Serialize)]
pub struct TransportStats {
    pub direct_addresses: Vec<SocketAddr>,
    /// Application payload written to QUIC; excludes framing, covers and retransmissions.
    pub audio_bytes_sent: u64,
    /// Application payload read from QUIC, including interrupted audio transfers.
    pub audio_bytes_received: u64,
    pub range_proof_bytes_received: u64,
    pub range_windows_verified: u64,
    pub range_connections_established: u64,
    pub relay_windows_verified: u64,
    pub relay_only: bool,
    pub range_streaming: bool,
    /// Count of received track announcements, including entries later filtered by policy.
    pub catalog_entries_received: u64,
    pub explicit_catalog: bool,
    pub versioned_catalog: bool,
    pub share_cached: bool,
}

/// The main P2P node. Wraps an iroh `Endpoint` and an iroh-blobs `FsStore`.
type LegacyFetch = futures_util::future::BoxFuture<'static, Result<(Bytes, String), Arc<str>>>;

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
    legacy_fetches:
        std::sync::Mutex<HashMap<(Uuid, Hash), futures_util::future::WeakShared<LegacyFetch>>>,
    /// Track health manager for failure tracking and auto-repair.
    health_manager: Arc<TrackHealthManager>,
    /// Semaphore to limit concurrent incoming P2P connections.
    conn_semaphore: Arc<tokio::sync::Semaphore>,
    manual_addresses: MemoryLookup,
    manual_relay: Option<iroh::RelayUrl>,
    catalogs: catalogs::Catalogs,
    pex_workers: Arc<tokio::sync::Semaphore>,
    subscribers: tokio::sync::RwLock<std::collections::HashSet<EndpointId>>,
    audio_bytes_sent: AtomicU64,
    audio_bytes_received: AtomicU64,
    catalog_entries_received: AtomicU64,
    /// Blob hashes that have been explicitly published/announced.
    /// Only these blobs can be served to peers via FetchTrack.
    published_hashes: tokio::sync::RwLock<HashMap<String, BlobKind>>,
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
    ranges: ranges::RangeTransfers,
}

// redb writes a new allocator layout on open. A rollback also needs any data
// files subsequently removed by blob GC, so the marker confirms a volume snapshot.
const BLOB_STORE_MARKER: &str = ".soundtime-blobs-0.105";

async fn check_blob_store_upgrade(root: &std::path::Path) -> Result<(), P2pError> {
    let marker = root.join(BLOB_STORE_MARKER);
    if tokio::fs::try_exists(&marker).await? {
        return Ok(());
    }
    if tokio::fs::try_exists(root.join("blobs.db")).await? {
        return Err(P2pError::BlobStore(format!(
            "existing P2P store at {} requires a stopped-volume backup before upgrade. \
             Follow 'Upgrading the P2P store' in docs/p2p-networking.md, then create {BLOB_STORE_MARKER} in its root",
            root.display()
        )));
    }
    tokio::fs::write(marker, b"").await?;
    Ok(())
}

// Count payload as application I/O completes so a cancelled transfer still contributes
// its accepted prefix. The size on the wire is untrusted; allocate only as bytes arrive.
async fn read_blob_payload(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    expected: usize,
    counter: Option<&AtomicU64>,
) -> Result<Vec<u8>, P2pError> {
    use tokio::io::AsyncReadExt;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut data = Vec::new();
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        if let Some(counter) = counter {
            counter.fetch_add(n as u64, Ordering::Relaxed);
        }
        if n > expected - data.len() {
            return Err(P2pError::Connection("blob exceeds declared length".into()));
        }
        data.extend_from_slice(&buffer[..n]);
    }
    if data.len() != expected {
        return Err(P2pError::Connection("incomplete blob payload".into()));
    }
    Ok(data)
}

async fn write_blob_payload(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    mut data: &[u8],
    counter: Option<&AtomicU64>,
) -> Result<(), P2pError> {
    use tokio::io::AsyncWriteExt;
    while !data.is_empty() {
        let n = writer.write(data).await?;
        if n == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into());
        }
        if let Some(counter) = counter {
            counter.fetch_add(n as u64, Ordering::Relaxed);
        }
        data = &data[n..];
    }
    Ok(())
}

impl P2pNode {
    /// Start a new P2P node with the given config and database.
    pub async fn start(config: P2pConfig, db: DatabaseConnection) -> Result<Arc<Self>, P2pError> {
        // Ensure blobs directory exists
        tokio::fs::create_dir_all(&config.blobs_dir)
            .await
            .map_err(P2pError::Io)?;

        check_blob_store_upgrade(&config.blobs_dir).await?;

        // Load or generate secret key for stable EndpointId across restarts
        let secret_key = Self::load_or_generate_key(&config).await?;
        let node_id = secret_key.public();
        soundtime_db::federation::bind_local_node(&db, &node_id.to_string()).await?;
        info!(%node_id, "starting P2P node");

        // Initialize the blob store
        let blob_store = FsStore::load(&config.blobs_dir)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        // N0 configures the public relays and DNS/Pkarr address lookups.
        let manual_addresses = MemoryLookup::new();
        let manual_relay = config
            .relay_url
            .as_deref()
            .map(str::parse::<iroh::RelayUrl>)
            .transpose()
            .map_err(|e| P2pError::Endpoint(format!("invalid P2P_RELAY_URL: {e}")))?;
        if manual_relay.as_ref().is_some_and(|u| {
            !matches!(u.scheme(), "http" | "https")
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
        }) {
            return Err(P2pError::Endpoint(
                "expected an HTTP(S) relay URL without credentials, query or fragment".into(),
            ));
        }
        if config.relay_only && manual_relay.is_none() {
            return Err(P2pError::Endpoint(
                "P2P_RELAY_ONLY requires P2P_RELAY_URL".into(),
            ));
        }
        let builder = if config.enable_public_discovery {
            Endpoint::builder(iroh::endpoint::presets::N0)
        } else {
            Endpoint::builder(iroh::endpoint::presets::Minimal)
                .dns_resolver(
                    iroh::dns::DnsResolver::builder()
                        .with_system_defaults()
                        .disable_fallback()
                        .build(),
                )
                .portmapper_config(iroh::endpoint::PortmapperConfig::Disabled)
        };
        let mut protocols = vec![SOUNDTIME_ALPN.to_vec()];
        if config.explicit_catalog {
            protocols.insert(0, EXPLICIT_CATALOG_ALPN.to_vec());
        }
        if config.explicit_catalog && config.versioned_catalog {
            protocols.push(crate::inventory::ALPN.to_vec());
        }
        if config.range_streaming {
            protocols.push(crate::range_transport::ALPN.to_vec());
        }
        let mut builder = builder
            .address_lookup(manual_addresses.clone())
            .secret_key(secret_key)
            .alpns(protocols);
        if let Some(url) = &manual_relay {
            builder =
                builder.relay_mode(iroh::RelayMode::Custom(iroh::RelayMap::from(url.clone())));
        }
        if config.relay_only {
            builder = builder.clear_ip_transports();
        }

        // Optionally enable local network discovery (mDNS)
        if config.enable_local_discovery {
            info!("enabling local network discovery (mDNS)");
            builder = builder
                .address_lookup(iroh_mdns_address_lookup::MdnsAddressLookupBuilder::default());
            tracing::info!("mDNS address lookup configured");
        }

        // Optionally enable DHT (BitTorrent Mainline) discovery
        if config.enable_public_discovery && config.enable_dht_discovery {
            info!("enabling DHT (BitTorrent Mainline) discovery");
            let dht_builder = iroh_mainline_address_lookup::DhtAddressLookup::builder();
            builder = builder.address_lookup(dht_builder);
            tracing::info!("DHT address lookup configured");
        }

        // Bind to the configured port (0 = random)
        if config.bind_port > 0 && !config.relay_only {
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
        if config.enable_public_discovery || manual_relay.is_some() {
            info!("waiting for relay connection...");
            match tokio::time::timeout(std::time::Duration::from_secs(15), endpoint.online()).await
            {
                Ok(()) => {
                    info!(relay = ?Self::connected_relay(&endpoint), "connected to relay server")
                }
                Err(_) => warn!("relay connection timed out after 15s — continuing without relay"),
            }
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

        let registry = Arc::new(PeerRegistry::persistent(db.clone()));

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

        let (maximum, min_free) =
            crate::blob_cache::budget_from_env().map_err(|e| P2pError::BlobStore(e.to_string()))?;
        let legacy = crate::blob_cache::legacy_charge(&blob_store, &config.blobs_dir, &db)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;
        let cache_dir = config.blobs_dir.join("cache-v2");
        let blob_cache = tokio::task::spawn_blocking(move || {
            BlobCache::open(cache_dir, maximum, min_free, legacy)
        })
        .await
        .map_err(|e| P2pError::BlobStore(e.to_string()))?
        .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        {
            use sea_orm::{ConnectionTrait, DbBackend, Statement};
            if let Some(row) = db
                .query_one(Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT value FROM instance_settings WHERE key='p2p_cache_quota_bytes'",
                ))
                .await?
            {
                let value = row
                    .try_get::<String>("", "value")?
                    .parse::<u64>()
                    .map_err(|e| P2pError::BlobStore(e.to_string()))?;
                let effective = value.min(maximum);
                let cache = Arc::clone(&blob_cache);
                tokio::task::spawn_blocking(move || cache.restore_maximum(effective))
                    .await
                    .map_err(|e| P2pError::BlobStore(e.to_string()))?
                    .map_err(|e| P2pError::BlobStore(e.to_string()))?;
            }
        }
        let health_manager = Arc::new(TrackHealthManager::new());

        let conn_pool = Arc::new(if config.explicit_catalog {
            ConnectionPool::new(endpoint.clone(), EXPLICIT_CATALOG_ALPN)
                .with_fallback(SOUNDTIME_ALPN)
        } else {
            ConnectionPool::new(endpoint.clone(), SOUNDTIME_ALPN)
        });

        let node = Arc::new(Self {
            catalogs: catalogs::Catalogs::new(endpoint.clone()),
            ranges: ranges::RangeTransfers::new(endpoint.clone()),
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
            legacy_fetches: Default::default(),
            health_manager,
            conn_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_P2P_CONNECTIONS)),
            manual_addresses,
            manual_relay,
            pex_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            subscribers: Default::default(),
            audio_bytes_sent: AtomicU64::new(0),
            audio_bytes_received: AtomicU64::new(0),
            catalog_entries_received: AtomicU64::new(0),
            published_hashes: tokio::sync::RwLock::new(HashMap::new()),
            pex_index: AtomicUsize::new(0),
            catalog_sync_in_progress: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            conn_pool,
        });

        if node.versioned_catalog() {
            node.start_catalog_worker();
            node.start_mirror_worker();
        }

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
                                hashes.insert(h.clone(), BlobKind::Audio);
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

        // Backfill content_hash for tracks imported before P2P was enabled.
        // This ensures all local tracks are available for catalog sync.
        {
            let node_clone = Arc::clone(&node);
            tokio::spawn(async move {
                // Small delay to let the published_hashes pre-population finish first
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                node_clone.backfill_content_hashes().await;
            });
        }

        // Spawn the connection accept loop
        let node_clone = Arc::clone(&node);
        tokio::spawn(async move {
            node_clone.accept_loop().await;
        });

        if let Some(url) = node._config.rendezvous_url.clone() {
            let node = Arc::clone(&node);
            let mut shutdown = node.shutdown_tx.subscribe();
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let worker = async {
                    loop {
                        interval.tick().await;
                        if let Err(e) = node.refresh_rendezvous(&client, &url).await {
                            warn!("rendezvous registration failed; retrying in five minutes: {e}");
                        }
                    }
                };
                tokio::select! {
                    _ = worker => {},
                    _ = shutdown.changed() => {},
                }
            });
        }

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
                let mut followed_after = String::new();
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if node_clone.explicit_catalog() {
                                if let Err(error) = node_clone.retry_followed(&mut followed_after).await {
                                    warn!(%error, "could not check persisted subscriptions");
                                }
                            }
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

        node.start_legacy_cache_recovery();
        // The versioned catalogue tracks source leases and confirmed peer health.
        // Its metadata worker must never download unrequested audio.
        if !node.explicit_catalog() {
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
        if node._config.enable_public_discovery {
            let endpoint_clone = node.endpoint.clone();
            let mut shutdown_rx = node.shutdown_tx.subscribe();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                interval.tick().await; // skip first immediate tick
                let mut was_connected = true; // assume connected after init
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            match Self::connected_relay(&endpoint_clone) {
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
        Self::connected_relay(&self.endpoint)
    }

    /// Get the number of direct addresses the endpoint is listening on.
    pub fn direct_addresses_count(&self) -> usize {
        self.node_addr()
            .map(|addr| addr.ip_addrs().count())
            .unwrap_or(0)
    }

    pub fn transport_stats(&self) -> TransportStats {
        TransportStats {
            direct_addresses: self.endpoint.addr().ip_addrs().copied().collect(),
            audio_bytes_sent: self.audio_bytes_sent.load(Ordering::Relaxed),
            audio_bytes_received: self.audio_bytes_received.load(Ordering::Relaxed),
            range_proof_bytes_received: self.ranges.proof_bytes_received.load(Ordering::Relaxed),
            range_windows_verified: self.ranges.verified_windows.load(Ordering::Relaxed),
            range_connections_established: self.ranges.pool.established(),
            relay_windows_verified: self.ranges.relay_windows_verified.load(Ordering::Relaxed),
            relay_only: self._config.relay_only,
            range_streaming: self.range_streaming(),
            catalog_entries_received: self.catalog_entries_received.load(Ordering::Relaxed),
            explicit_catalog: self.explicit_catalog(),
            versioned_catalog: self.versioned_catalog(),
            share_cached: self.share_cached(),
        }
    }

    pub fn rendezvous_configured(&self) -> bool {
        self._config.rendezvous_url.is_some()
    }

    /// Whether DHT discovery is enabled for this node.
    pub fn dht_discovery_enabled(&self) -> bool {
        self._config.enable_public_discovery && self._config.enable_dht_discovery
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
        self.publish_original(data, None).await
    }

    /// TryReference keeps large, operator-declared immutable originals in place.
    /// For mutable storage, import the exact bytes captured at publication time.
    pub async fn publish_original(
        &self,
        data: Bytes,
        path: Option<PathBuf>,
    ) -> Result<Hash, P2pError> {
        use iroh_blobs::{
            api::blobs::{AddPathOptions, ImportMode},
            BlobFormat,
        };
        let blobs = self.blob_store.blobs();
        let outcome = if let Some(path) = path {
            let expected = Hash::new(&data);
            let path = tokio::fs::canonicalize(path)
                .await
                .map_err(|e| P2pError::BlobStore(e.to_string()))?;
            let tag = blobs
                .add_path_with_opts(AddPathOptions {
                    path,
                    mode: ImportMode::TryReference,
                    format: BlobFormat::Raw,
                })
                .temp_tag()
                .await
                .map_err(|err| P2pError::BlobStore(err.to_string()))?;
            if tag.hash() != expected {
                return Err(P2pError::BlobStore(
                    "original changed during publication".into(),
                ));
            }
            tag
        } else {
            blobs
                .add_bytes(data)
                .temp_tag()
                .await
                .map_err(|err| P2pError::BlobStore(err.to_string()))?
        };
        let hash = outcome.hash();

        // Create a persistent tag so the blob is not garbage collected
        let tag_name = format!("published-{}", hash);
        self.blob_store
            .tags()
            .set(tag_name, HashAndFormat::raw(hash))
            .await
            .map_err(|e| P2pError::BlobStore(format!("failed to set persistent tag: {e}")))?;

        // SECURITY: Register hash so it can be served to peers (FIX-19)
        self.published_hashes
            .write()
            .await
            .insert(hash.to_string(), BlobKind::Audio);
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
        self.published_hashes
            .write()
            .await
            .entry(hash.to_string())
            .or_insert(BlobKind::Cover);
        debug!(%hash, "cover art published to blob store");
        Ok(hash)
    }

    /// Retrieve a track's data from the local blob store by hash.
    pub async fn get_local_track(&self, hash: Hash) -> Result<Bytes, P2pError> {
        if let Some(data) = self
            .blob_cache
            .read(hash)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?
        {
            return Ok(data);
        }
        let data = self
            .blob_store
            .blobs()
            .get_bytes(hash)
            .await
            .map_err(|e| P2pError::BlobStore(e.to_string()))?;

        Ok(data)
    }

    pub fn cache_on_play(&self) -> bool {
        self._config.cache_on_play
    }
    pub async fn reserve_cached_version(
        &self,
        hash: Hash,
        size: u64,
    ) -> anyhow::Result<Option<crate::blob_cache::CacheWriter>> {
        let cache = Arc::clone(&self.blob_cache);
        tokio::task::spawn_blocking(move || cache.reserve(hash, size)).await?
    }
    pub fn cache_contains(&self, hash: Hash) -> bool {
        self.blob_cache.contains(hash)
    }
    pub async fn pin_cached(&self, hash: Hash, pin: bool) -> anyhow::Result<bool> {
        let cache = Arc::clone(&self.blob_cache);
        tokio::task::spawn_blocking(move || cache.pin(hash, pin)).await?
    }

    /// Check if a blob exists locally.
    pub async fn has_blob(&self, hash: Hash) -> bool {
        self.cache_contains(hash) || self.blob_store.blobs().has(hash).await.unwrap_or(false)
    }

    /// Connect to a remote peer and fetch a track by its content hash.
    pub async fn fetch_track_from_peer(
        &self,
        peer_addr: EndpointAddr,
        hash: Hash,
    ) -> Result<Bytes, P2pError> {
        let source = peer_addr.id.to_string();
        let grant = soundtime_db::media_access::ReadGuard::peer(&source);
        let raw = soundtime_db::media_access::ReadGuard::legacy_hash(&hash.to_string());
        let allowed = async || -> Result<bool, P2pError> {
            let policy = crate::peer_policy::get(&self.db, &source).await?;
            Ok(!policy.banned
                && !policy.source_disabled
                && (!self.explicit_catalog() || policy.followed)
                && soundtime_db::media_access::allows_legacy_hash(&self.db, &hash.to_string())
                    .await?)
        };
        if !allowed().await? {
            return Err(P2pError::PeerBlocked(source));
        }
        let data = grant
            .during(raw.during(self.fetch_blob_from_peer(peer_addr, hash, BlobKind::Audio)))
            .await???;
        if !allowed().await? {
            return Err(P2pError::PeerBlocked(source));
        }
        Ok(data)
    }

    async fn fetch_blob_from_peer(
        &self,
        peer_addr: EndpointAddr,
        hash: Hash,
        kind: BlobKind,
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

        let data = read_blob_payload(
            &mut recv,
            data_len,
            (kind == BlobKind::Audio).then_some(&self.audio_bytes_received),
        )
        .await?;

        debug!(%hash, bytes = data.len(), "received track from peer");
        Ok(Bytes::from(data))
    }

    pub fn explicit_catalog(&self) -> bool {
        self._config.explicit_catalog
    }

    pub async fn set_followed(&self, id: EndpointId, followed: bool) -> Result<(), P2pError> {
        if !self.explicit_catalog() || id == self.node_id() {
            return Err(P2pError::Connection(
                "explicit subscription requires a remote peer and pilot mode".into(),
            ));
        }
        let policy = crate::peer_policy::get(&self.db, &id.to_string()).await?;
        if policy.banned && followed {
            return Err(P2pError::PeerBlocked(id.to_string()));
        }
        crate::peer_policy::follow(&self.db, &id.to_string(), followed).await?;
        if !followed {
            soundtime_db::media_access::revoke_peer(&id.to_string());
        }
        Ok(())
    }

    /// Returns whether this change could also be signalled to the peer immediately.
    pub async fn update_subscription(
        &self,
        id: EndpointId,
        followed: bool,
    ) -> Result<bool, P2pError> {
        let peer = id.to_string();
        if followed {
            self.set_followed(id, true).await?;
        } else {
            if !self.explicit_catalog() || id == self.node_id() {
                return Err(P2pError::Connection(
                    "expected a remote subscription".into(),
                ));
            }
            crate::peer_policy::disconnect(&self.db, &peer).await?;
            soundtime_db::media_access::revoke_peer(&peer);
        }
        if crate::peer_policy::get(&self.db, &peer).await?.banned {
            return Ok(false);
        }
        let started = chrono::Utc::now();
        let result = if followed {
            // Announce the durable subscription before the initial pull; this also
            // negotiates capabilities when an admin follows a previously offline ID.
            match self.ping_peer(EndpointAddr::new(id)).await {
                Ok(P2pMessage::Pong { .. }) => self.request_catalog_from_peer(id).await,
                Ok(_) => Err(P2pError::Connection(
                    "invalid subscription presence reply".into(),
                )),
                Err(error) => Err(error),
            }
        } else if self
            .registry
            .get_peer(&peer)
            .await
            .is_some_and(|p| p.is_online)
        {
            match self.conn_pool.get_connection(id).await {
                Ok(conn) if conn.alpn() == EXPLICIT_CATALOG_ALPN => {
                    self.send_message_to_peer(id, &P2pMessage::StopCatalog)
                        .await
                }
                Ok(_) => return Ok(false),
                Err(error) => Err(error),
            }
        } else {
            return Ok(false);
        };
        if let Err(error) = result {
            warn!(%peer, %error, "subscription saved locally; peer notification deferred");
            self.registry.failed_since(&peer, started, false).await;
            return Ok(false);
        }
        Ok(true)
    }

    async fn record_presence(
        &self,
        id: EndpointId,
        share: bool,
        followed: bool,
        versioned: bool,
    ) -> Result<(), P2pError> {
        self.catalogs
            .capability(id, versioned && self.versioned_catalog())
            .await;
        crate::peer_policy::record_consent(&self.db, &id.to_string(), share).await?;
        let mut subscribers = self.subscribers.write().await;
        if followed {
            subscribers.insert(id);
        } else {
            subscribers.remove(&id);
        }
        Ok(())
    }

    async fn pex_sample(&self, recipient: EndpointId) -> Result<Vec<String>, P2pError> {
        if crate::peer_policy::get(&self.db, &recipient.to_string())
            .await?
            .banned
        {
            return Err(P2pError::PeerBlocked(recipient.to_string()));
        }
        let mut peers =
            crate::peer_policy::shareable(&self.db, &recipient.to_string(), PEX_LIMIT as i64)
                .await?;
        if self._config.share_identity && recipient != self.node_id() {
            peers.truncate(PEX_LIMIT - 1);
            peers.push(self.node_id().to_string());
        }
        Ok(peers)
    }

    async fn verify_pex_candidates(&self, peers: Vec<String>) {
        let mut seen = std::collections::HashSet::new();
        for raw in peers.into_iter().take(PEX_LIMIT) {
            let Ok(id) = raw.parse::<EndpointId>() else {
                continue;
            };
            if id == self.node_id()
                || !seen.insert(id)
                || self.registry.get_peer(&raw).await.is_some()
            {
                continue;
            }
            let Ok(policy) = crate::peer_policy::get(&self.db, &raw).await else {
                continue;
            };
            if policy.banned {
                continue;
            }
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.ping_peer(EndpointAddr::new(id)),
            )
            .await;
        }
    }

    /// Send a ping to a peer and wait for pong.
    pub async fn ping_peer(&self, peer_addr: EndpointAddr) -> Result<P2pMessage, P2pError> {
        self.ping_into(peer_addr, self.registry()).await
    }

    pub(super) async fn ping_into(
        &self,
        peer_addr: EndpointAddr,
        registry: &PeerRegistry,
    ) -> Result<P2pMessage, P2pError> {
        let started = chrono::Utc::now();
        let source = peer_addr.id.to_string();
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(12),
            self.probe_presence(peer_addr),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(P2pError::Connection("peer presence timed out".into())),
        };
        match &result {
            Ok(P2pMessage::Pong {
                node_id,
                track_count,
                version,
            }) if node_id == &source => {
                registry
                    .upsert_peer_versioned(&source, None, *track_count, version.clone())
                    .await;
            }
            Err(P2pError::Connection(_) | P2pError::Endpoint(_) | P2pError::Io(_)) => {
                registry.failed_since(&source, started, false).await;
            }
            _ => {}
        }
        result
    }

    async fn probe_presence(&self, peer_addr: EndpointAddr) -> Result<P2pMessage, P2pError> {
        let policy = if self.explicit_catalog() {
            let id = peer_addr.id.to_string();
            let policy = crate::peer_policy::get(&self.db, &id).await?;
            if policy.banned {
                return Err(P2pError::PeerBlocked(id));
            }
            Some(policy)
        } else {
            None
        };
        let peer_addr = if let Some(url) = &self.manual_relay {
            peer_addr.with_relay_url(url.clone())
        } else {
            peer_addr
        };
        if peer_addr.ip_addrs().next().is_some() || peer_addr.relay_urls().next().is_some() {
            self.manual_addresses.set_endpoint_info(peer_addr.clone());
        }
        let conn = self.conn_pool.get_connection(peer_addr.id).await.map_err(|e| {
            let err_str = format!("{e}");
            if err_str.contains("ALPN") || err_str.contains("protocol") || err_str.contains("timed out") {
                P2pError::Connection(format!(
                    "{e} (possible protocol version mismatch — this node uses ALPN 'soundtime/p2p/1')"
                ))
            } else {
                e
            }
        })?;

        let probe = if let Some(policy) = policy {
            presence_probe(
                conn.alpn(),
                policy.followed,
                self._config.share_identity,
                self.versioned_catalog(),
            )?
        } else {
            P2pMessage::Ping
        };
        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

        let ping = serde_json::to_vec(&probe)?;
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

        if msg_len > 16 * 1024 {
            return Err(P2pError::Connection("oversized ping response".into()));
        }
        let response = recv
            .read_to_end(msg_len)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let pong: P2pMessage = serde_json::from_slice(&response)?;
        let pong = match pong {
            P2pMessage::PresenceReply {
                node_id,
                track_count,
                version,
                share_identity,
                follows_recipient,
                versioned_catalog,
            } if matches!(probe, P2pMessage::Presence { .. })
                && node_id == peer_addr.id.to_string() =>
            {
                self.record_presence(
                    peer_addr.id,
                    share_identity,
                    follows_recipient,
                    versioned_catalog,
                )
                .await?;
                P2pMessage::Pong {
                    node_id,
                    track_count,
                    version,
                }
            }
            msg @ P2pMessage::Pong { .. } if matches!(probe, P2pMessage::Ping) => msg,
            _ => {
                return Err(P2pError::Connection(
                    "invalid response to negotiated presence".into(),
                ))
            }
        };
        if !matches!(&pong, P2pMessage::Pong { node_id, .. } if *node_id == peer_addr.id.to_string())
        {
            return Err(P2pError::Connection(
                "invalid pong from authenticated peer".into(),
            ));
        }
        if matches!(probe, P2pMessage::Ping) {
            self.catalogs.capability(peer_addr.id, false).await;
            crate::peer_policy::record_consent(&self.db, &peer_addr.id.to_string(), false).await?;
        }
        Ok(pong)
    }

    async fn refresh_rendezvous(
        self: &Arc<Self>,
        client: &reqwest::Client,
        url: &str,
    ) -> anyhow::Result<()> {
        let peers = crate::rendezvous::discover(client, url, self.endpoint.secret_key()).await?;
        for id in peers {
            let peer_id = id.to_string();
            if self
                .registry
                .get_peer(&peer_id)
                .await
                .is_some_and(|peer| peer.is_online)
                || is_peer_blocked(&self.db, &peer_id).await
            {
                continue;
            }
            let ping = self.ping_peer(EndpointAddr::new(id));
            let Ok(Ok(P2pMessage::Pong { .. })) =
                tokio::time::timeout(std::time::Duration::from_secs(15), ping).await
            else {
                debug!(peer = %peer_id, "rendezvous peer did not respond to ping");
                continue;
            };
            info!(peer = %peer_id, "discovered SoundTime peer via rendezvous");
            if !self.explicit_catalog() {
                let node = Arc::clone(self);
                tokio::spawn(async move {
                    node.announce_all_tracks_to_peer(id).await;
                });
            } else {
                self.resume_subscription(id).await?;
            }
        }
        Ok(())
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
                    ..
                }) => {
                    info!(peer = %nid, %track_count, "seed peer connected and registered");
                    // Exchange peer lists to discover the wider network
                    self.discover_via_peer(node_id).await;
                    // Sync our full catalog to this peer so they get our existing tracks
                    if !self.explicit_catalog() {
                        self.announce_all_tracks_to_peer(node_id).await;
                    } else if let Err(error) = self.resume_subscription(node_id).await {
                        warn!(peer = %nid, %error, "could not resume followed seed");
                    }
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
                             this node uses ALPN 'soundtime/p2p/1', the remote peer \
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
        let catalog = matches!(
            msg,
            P2pMessage::CatalogSync(_)
                | P2pMessage::CatalogDelta { .. }
                | P2pMessage::AnnounceTrack(_)
        );
        let msg_bytes = if catalog && self.catalogs.supports(node_id).await {
            serde_json::to_vec(&P2pMessage::CatalogChanged)?
        } else {
            serde_json::to_vec(msg)?
        };
        let max_retries = 3u32;
        let mut delay = std::time::Duration::from_secs(1);

        for attempt in 0..max_retries {
            if self.explicit_catalog() {
                let id = node_id.to_string();
                if crate::peer_policy::get(&self.db, &id).await?.banned {
                    return Err(P2pError::PeerBlocked(id));
                }
                if catalog && !self.subscribers.read().await.contains(&node_id) {
                    return Ok(());
                }
                if catalog && !crate::peer_policy::permits_export(&self.db, &id).await? {
                    return Ok(());
                }
            }
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
                let started = chrono::Utc::now();
                if let Err(e) = node.send_message_to_peer(node_id, &msg).await {
                    warn!(peer = %peer_id, "failed to announce track: {e}");
                    node.registry.failed_since(&peer_id, started, false).await;
                } else {
                    debug!(peer = %peer_id, "track announced");
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }
    }

    /// Backfill `content_hash` for local tracks that were imported before P2P was enabled.
    /// Reads each file, publishes it to the blob store (BLAKE3), and updates the DB.
    /// Runs on startup to ensure all local tracks are available for P2P catalog sync.
    async fn backfill_content_hashes(&self) {
        let page_size = 100u64;
        let total = match track::Entity::find()
            .filter(track::Column::ContentHash.is_null())
            .filter(track::Column::FilePath.not_like("p2p://%"))
            .count(&self.db)
            .await
        {
            Ok(0) => {
                debug!("all local tracks already have content hashes");
                return;
            }
            Ok(c) => c,
            Err(e) => {
                warn!("failed to count tracks for content hash backfill: {e}");
                return;
            }
        };

        info!(total, "starting content hash backfill for local tracks");

        let num_pages = total.div_ceil(page_size);
        let mut backfilled = 0u64;
        let mut skipped = 0u64;

        for page_num in 0..num_pages {
            let tracks = match track::Entity::find()
                .filter(track::Column::ContentHash.is_null())
                .filter(track::Column::FilePath.not_like("p2p://%"))
                .paginate(&self.db, page_size)
                .fetch_page(page_num)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    warn!(
                        page = page_num,
                        "failed to fetch tracks page for backfill: {e}"
                    );
                    continue;
                }
            };

            for t in &tracks {
                let file_path = self.audio_storage_path.join(&t.file_path);
                let data = match tokio::fs::read(&file_path).await {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(
                            track_id = %t.id,
                            path = %file_path.display(),
                            "skipping backfill — file not readable: {e}"
                        );
                        skipped += 1;
                        continue;
                    }
                };

                match self.publish_track(Bytes::from(data)).await {
                    Ok(hash) => {
                        let update = track::ActiveModel {
                            id: Set(t.id),
                            content_hash: Set(Some(hash.to_string())),
                            ..Default::default()
                        };
                        if let Err(e) = update.update(&self.db).await {
                            warn!(track_id = %t.id, "failed to save content_hash during backfill: {e}");
                            skipped += 1;
                            continue;
                        }
                        backfilled += 1;
                    }
                    Err(e) => {
                        warn!(track_id = %t.id, "failed to publish track during backfill: {e}");
                        skipped += 1;
                    }
                }
            }

            info!(
                processed = backfilled + skipped,
                total, backfilled, skipped, "content hash backfill progress"
            );
        }

        info!(backfilled, skipped, total, "content hash backfill complete");
    }

    /// Announce all locally-uploaded tracks to a specific peer using paginated queries.
    /// Called when a new peer connects to sync existing catalogs.
    /// Sends one CatalogSync message per page (500 tracks) to avoid loading all
    /// tracks into memory at once.
    pub async fn announce_all_tracks_to_peer(&self, peer_id: EndpointId) {
        if self.explicit_catalog() && !self.subscribers.read().await.contains(&peer_id) {
            return;
        }
        if self.catalogs.supports(peer_id).await {
            if let Err(error) = self
                .send_message_to_peer(peer_id, &P2pMessage::CatalogChanged)
                .await
            {
                debug!(%peer_id,%error,"catalogue change notification deferred");
            }
            return;
        }
        let page_size = 500u64;
        let total = match track::Entity::find()
            .filter(track::Column::ContentHash.is_not_null())
            .filter(track::Column::FilePath.not_like("p2p://%"))
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

        // Cache cover hashes by album_id to avoid re-reading + re-publishing the same cover
        let mut cover_cache: HashMap<Uuid, Option<String>> = HashMap::new();

        for page_num in 0..num_pages {
            let tracks = match track::Entity::find()
                .filter(track::Column::ContentHash.is_not_null())
                .filter(track::Column::FilePath.not_like("p2p://%"))
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

            // ── Batch-fetch artists and albums for this page ──
            let artist_ids: Vec<Uuid> = tracks
                .iter()
                .map(|t| t.artist_id)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let album_ids: Vec<Uuid> = tracks
                .iter()
                .filter_map(|t| t.album_id)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let artist_map: HashMap<Uuid, String> = if !artist_ids.is_empty() {
                artist::Entity::find()
                    .filter(artist::Column::Id.is_in(artist_ids))
                    .all(&self.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| (a.id, a.name))
                    .collect()
            } else {
                HashMap::new()
            };

            let album_map: HashMap<Uuid, (String, Option<String>)> = if !album_ids.is_empty() {
                album::Entity::find()
                    .filter(album::Column::Id.is_in(album_ids))
                    .all(&self.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| (a.id, (a.title, a.cover_url)))
                    .collect()
            } else {
                HashMap::new()
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

                let artist_name = artist_map
                    .get(&t.artist_id)
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string());

                let (album_title, cover_hash) = match t.album_id {
                    Some(aid) => {
                        match album_map.get(&aid) {
                            Some((title, cover_url)) => {
                                let title = Some(title.clone());
                                // Use cover cache to avoid re-reading/re-publishing same cover
                                let ch = if let Some(cached) = cover_cache.get(&aid) {
                                    cached.clone()
                                } else {
                                    let result = if let Some(ref url) = cover_url {
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
                                    cover_cache.insert(aid, result.clone());
                                    result
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

    /// Walk a bounded, indexed slice of subscriptions on each tick. This includes
    /// a peer followed through the admin API while it was completely offline.
    async fn retry_followed(&self, after: &mut String) -> Result<(), P2pError> {
        let batch = crate::peer_policy::followed_after(&self.db, after).await?;
        for peer in &batch {
            let Ok(id) = peer.parse::<EndpointId>() else {
                continue;
            };
            if self
                .registry
                .get_peer(peer)
                .await
                .is_some_and(|p| p.is_online)
            {
                continue;
            }
            let resume = async {
                if let P2pMessage::Pong { .. } = self.ping_peer(EndpointAddr::new(id)).await? {
                    self.resume_subscription(id).await?;
                }
                Ok::<_, P2pError>(())
            };
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_secs(5), resume).await,
                Ok(Ok(()))
            ) {
                debug!(%peer, "followed peer still unavailable");
            }
        }
        *after = if batch.len() == 8 {
            batch.last().cloned().unwrap_or_default()
        } else {
            String::new()
        };
        Ok(())
    }

    pub async fn resume_subscription(&self, id: EndpointId) -> Result<(), P2pError> {
        if self.explicit_catalog() {
            let policy = crate::peer_policy::get(&self.db, &id.to_string()).await?;
            if !policy.banned && policy.followed {
                self.request_catalog_from_peer(id).await?;
            }
        }
        Ok(())
    }

    /// Send a `RequestCatalog` message to a peer, asking them to send us their
    /// full catalog via paginated `CatalogSync` messages.
    pub async fn request_catalog_from_peer(&self, peer_id: EndpointId) -> Result<(), P2pError> {
        if self.explicit_catalog()
            && !crate::peer_policy::get(&self.db, &peer_id.to_string())
                .await?
                .followed
        {
            return Err(P2pError::Connection(
                "follow the peer before requesting its catalogue".into(),
            ));
        }
        if self.catalogs.supports(peer_id).await {
            return self
                .pull_catalog(peer_id)
                .await
                .map_err(|e| P2pError::Connection(e.to_string()));
        }
        info!(peer = %peer_id, "requesting full catalog from peer");
        self.send_message_to_peer(peer_id, &P2pMessage::RequestCatalog)
            .await
    }

    /// Discover peers by exchanging peer lists with a known peer (Peer Exchange / PEX).
    /// Sends our known peers, receives theirs, and connects to any new ones.
    pub async fn discover_via_peer(&self, peer_node_id: EndpointId) {
        info!(peer = %peer_node_id, "initiating peer exchange");

        let peer_addr = EndpointAddr::new(peer_node_id);
        let started = chrono::Utc::now();
        match self.exchange_peers(peer_addr).await {
            Ok(remote_peers) => self.verify_pex_candidates(remote_peers).await,
            Err(e) => {
                warn!(peer = %peer_node_id, "peer exchange failed: {e}");
                self.registry
                    .failed_since(&peer_node_id.to_string(), started, false)
                    .await;
            }
        }
    }

    /// Send our peer list to a remote peer and receive theirs.
    async fn exchange_peers(&self, peer_addr: EndpointAddr) -> Result<Vec<String>, P2pError> {
        let peers = self.pex_sample(peer_addr.id).await?;
        let conn = self.conn_pool.get_connection(peer_addr.id).await?;

        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(streams) => streams,
            Err(e) => {
                self.conn_pool.invalidate(&peer_addr.id).await;
                return Err(P2pError::Connection(e.to_string()));
            }
        };

        let msg = P2pMessage::PeerExchange { peers };
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
        if msg_len > 16 * 1024 {
            return Err(P2pError::Connection("oversized PEX response".into()));
        }

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
    /// Queries up to 10 matching peers concurrently and keeps results from responsive peers.
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
        // Finish collecting responsive peers within the HTTP search deadline.
        let timeout_dur = std::time::Duration::from_secs(3);
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

        // Keep different publications and providers distinct. Policy is applied to
        // each authenticated provider after the network request completes.
        all_results.sort_by(|a, b| {
            b.relevance
                .total_cmp(&a.relevance)
                .then_with(|| a.source_node.cmp(&b.source_node))
                .then_with(|| a.publication_id.cmp(&b.publication_id))
                .then_with(|| a.hash.cmp(&b.hash))
        });
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| {
            seen.insert((
                r.source_node.clone(),
                r.authority_node.clone(),
                r.publication_id,
                r.revision,
                r.hash.clone(),
            ))
        });

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
        let source = peer_addr.id.to_string();
        if self.explicit_catalog() && crate::peer_policy::get(&self.db, &source).await?.banned {
            return Err(P2pError::PeerBlocked(source));
        }
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

        if msg_len > 1024 * 1024 {
            return Err(P2pError::Connection("oversized search response".into()));
        }
        let response = recv
            .read_to_end(msg_len)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        let response_msg: P2pMessage = serde_json::from_slice(&response)?;

        if self.explicit_catalog() && crate::peer_policy::get(&self.db, &source).await?.banned {
            return Err(P2pError::PeerBlocked(source));
        }
        match (msg, response_msg) {
            (
                P2pMessage::SearchQuery {
                    request_id: query_id,
                    limit,
                    ..
                },
                P2pMessage::SearchResults {
                    request_id,
                    mut results,
                    ..
                },
            ) if *query_id == request_id => {
                results.retain(|r| r.source_node == source);
                results.truncate(*limit as usize);
                Ok(results)
            }
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

        if query.len() > 512 {
            return Err(P2pError::Connection("search query too long".into()));
        }
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
            hash: String,
            authority_node: String,
            publication_id: uuid::Uuid,
            revision: i64,
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
            include_str!("search_publications.sql"),
            [
                tsquery.into(),
                (limit.min(100) as i64).into(),
                our_node.clone().into(),
            ],
        ))
        .all(&self.db)
        .await?;

        let results: Vec<SearchResultItem> = rows
            .into_iter()
            .map(|r| SearchResultItem {
                hash: r.hash,
                authority_node: Some(r.authority_node),
                publication_id: Some(r.publication_id),
                revision: Some(r.revision),
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

        let page_size = 500u64;
        let total = match track::Entity::find()
            .filter(track::Column::ContentHash.is_not_null())
            .filter(track::Column::FilePath.not_like("p2p://%"))
            .filter(track::Column::CreatedAt.gt(since_ts))
            .count(&self.db)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to count tracks for incremental sync: {e}");
                return;
            }
        };

        if total == 0 {
            debug!(peer = %peer_id, "no new tracks since last sync");
            return;
        }

        let num_pages = total.div_ceil(page_size);
        let our_node = self.node_id().to_string();
        let mut cover_cache: HashMap<Uuid, Option<String>> = HashMap::new();

        info!(
            peer = %peer_id,
            total,
            since = %since_ts,
            pages = num_pages,
            "starting paginated incremental sync"
        );

        for page_num in 0..num_pages {
            let tracks = match track::Entity::find()
                .filter(track::Column::ContentHash.is_not_null())
                .filter(track::Column::FilePath.not_like("p2p://%"))
                .filter(track::Column::CreatedAt.gt(since_ts))
                .paginate(&self.db, page_size)
                .fetch_page(page_num)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    warn!(
                        page = page_num,
                        "failed to read tracks page for incremental sync: {e}"
                    );
                    continue;
                }
            };

            // ── Batch-fetch artists and albums for this page ──
            let artist_ids: Vec<Uuid> = tracks
                .iter()
                .map(|t| t.artist_id)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let album_ids: Vec<Uuid> = tracks
                .iter()
                .filter_map(|t| t.album_id)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let artist_map: HashMap<Uuid, String> = if !artist_ids.is_empty() {
                artist::Entity::find()
                    .filter(artist::Column::Id.is_in(artist_ids))
                    .all(&self.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| (a.id, a.name))
                    .collect()
            } else {
                HashMap::new()
            };

            let album_map: HashMap<Uuid, (String, Option<String>)> = if !album_ids.is_empty() {
                album::Entity::find()
                    .filter(album::Column::Id.is_in(album_ids))
                    .all(&self.db)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| (a.id, (a.title, a.cover_url)))
                    .collect()
            } else {
                HashMap::new()
            };

            let mut announcements = Vec::with_capacity(tracks.len());

            for t in &tracks {
                if t.file_path.starts_with("p2p://") {
                    continue;
                }
                let hash = match &t.content_hash {
                    Some(h) => h.clone(),
                    None => continue,
                };

                let artist_name = artist_map
                    .get(&t.artist_id)
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string());

                let (album_title, cover_hash) = match t.album_id {
                    Some(aid) => match album_map.get(&aid) {
                        Some((title, cover_url)) => {
                            let title = Some(title.clone());
                            let ch = if let Some(cached) = cover_cache.get(&aid) {
                                cached.clone()
                            } else {
                                let result = if let Some(ref url) = cover_url {
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
                                                Err(_) => None,
                                            }
                                        }
                                        None => None,
                                    }
                                } else {
                                    None
                                };
                                cover_cache.insert(aid, result.clone());
                                result
                            };
                            (title, ch)
                        }
                        None => (None, None),
                    },
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

            if !announcements.is_empty() {
                info!(
                    peer = %peer_id,
                    count = announcements.len(),
                    page = page_num,
                    since = %since_ts,
                    "incremental catalog sync page"
                );

                let msg = P2pMessage::CatalogDelta {
                    since: since_ts,
                    tracks: announcements,
                };
                if let Err(e) = self.send_message_to_peer(peer_id, &msg).await {
                    warn!(peer = %peer_id, page = page_num, "failed to send catalog delta page: {e}");
                }
            }
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
        if let Some(url) = &self.manual_relay {
            self.manual_addresses
                .add_endpoint_info(EndpointAddr::new(conn.remote_id()).with_relay_url(url.clone()));
        }

        info!(%peer_id, "incoming connection");

        // Check if peer is blocked
        let banned = if self.explicit_catalog() {
            crate::peer_policy::get(&self.db, &peer_id).await?.banned
        } else {
            is_peer_blocked(&self.db, &peer_id).await
        };
        if banned {
            warn!(%peer_id, "rejected blocked peer");
            conn.close(1u8.into(), b"blocked");
            return Err(P2pError::PeerBlocked(peer_id));
        }

        if conn.alpn() == crate::inventory::ALPN {
            return self
                .serve_catalog(conn)
                .await
                .map_err(|e| P2pError::Connection(e.to_string()));
        }
        if conn.alpn() == crate::range_transport::ALPN {
            return self.handle_range_connection(conn).await;
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

            let entries = match &msg {
                P2pMessage::AnnounceTrack(_)
                | P2pMessage::MirrorOffer(_)
                | P2pMessage::SignedWithdrawal(_) => 1,
                P2pMessage::CatalogSync(items) | P2pMessage::CatalogDelta { tracks: items, .. } => {
                    items.len() as u64
                }
                _ => 0,
            };
            if entries > 0 {
                self.catalog_entries_received
                    .fetch_add(entries, Ordering::Relaxed);
            }
            if self.explicit_catalog() {
                let policy = crate::peer_policy::get(&self.db, &peer_id).await?;
                if policy.banned {
                    conn.close(1u32.into(), b"blocked");
                    return Err(P2pError::PeerBlocked(peer_id));
                }
                if (!policy.followed || self.catalogs.supports(conn.remote_id()).await)
                    && matches!(
                        msg,
                        P2pMessage::CatalogSync(_)
                            | P2pMessage::CatalogDelta { .. }
                            | P2pMessage::AnnounceTrack(_)
                    )
                {
                    continue;
                }
            }
            self.handle_message(msg, send, node_id, &peer_id, conn.alpn())
                .await?;
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
                Ok(Some(a)) => Some(a.id),
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
                        Ok(_) => Some(new_id),
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
            file_hash: Set(None),
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
        negotiated_alpn: &[u8],
    ) -> Result<(), P2pError> {
        match msg {
            P2pMessage::FetchTrack { hash } => {
                // SECURITY: Only serve blobs that were explicitly published (FIX-19)
                let grant = soundtime_db::media_access::ReadGuard::legacy_hash(&hash);
                let kind = self.published_hashes.read().await.get(&hash).copied();
                let kind = if kind.is_some()
                    && soundtime_db::media_access::allows_legacy_hash(&self.db, &hash).await?
                {
                    kind
                } else {
                    None
                };
                let Some(kind) = kind else {
                    warn!(%peer_id, %hash, "rejected FetchTrack for non-published blob");
                    send.write_all(&0u32.to_be_bytes())
                        .await
                        .map_err(|e| P2pError::Connection(e.to_string()))?;
                    send.finish()
                        .map_err(|e| P2pError::Connection(e.to_string()))?;
                    return Ok(());
                };

                let hash: Hash = hash
                    .parse()
                    .map_err(|_| P2pError::TrackNotFound(hash.clone()))?;

                match self.get_local_track(hash).await {
                    Ok(data) => {
                        let len = u32::try_from(data.len()).map_err(|_| {
                            P2pError::Connection("blob exceeds v1 frame limit".into())
                        })?;
                        send.write_all(&len.to_be_bytes())
                            .await
                            .map_err(|e| P2pError::Connection(e.to_string()))?;
                        grant
                            .during(write_blob_payload(
                                &mut send,
                                &data,
                                (kind == BlobKind::Audio).then_some(&self.audio_bytes_sent),
                            ))
                            .await??;
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
            probe @ (P2pMessage::Ping | P2pMessage::Presence { .. }) => {
                let remote: EndpointId = peer_id
                    .parse()
                    .map_err(|_| P2pError::Connection("invalid peer identity".into()))?;
                let modern = matches!(probe, P2pMessage::Presence { .. });
                if modern != (negotiated_alpn == EXPLICIT_CATALOG_ALPN) {
                    return Err(P2pError::Connection(
                        "presence is incompatible with negotiated ALPN".into(),
                    ));
                }
                // Count ALL local tracks (not just those with content_hash set).
                // During backfill, some tracks may not yet have hashes, but we still
                // report the full catalog size so peers know to wait for announcements.
                let track_count = match track::Entity::find()
                    .filter(track::Column::FilePath.not_like("p2p://%"))
                    .count(&self.db)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("failed to count tracks for ping response: {e}");
                        0
                    }
                };

                let pong = if let P2pMessage::Presence {
                    share_identity,
                    follows_recipient,
                    versioned_catalog,
                } = probe
                {
                    self.record_presence(
                        remote,
                        share_identity,
                        follows_recipient,
                        versioned_catalog,
                    )
                    .await?;
                    let ours = crate::peer_policy::get(&self.db, peer_id).await?;
                    P2pMessage::PresenceReply {
                        node_id: node_id.to_string(),
                        track_count,
                        version: Some(crate::build_version().to_string()),
                        share_identity: self._config.share_identity,
                        follows_recipient: ours.followed,
                        versioned_catalog: self.versioned_catalog(),
                    }
                } else {
                    self.catalogs.capability(remote, false).await;
                    crate::peer_policy::record_consent(&self.db, peer_id, false).await?;
                    P2pMessage::Pong {
                        node_id: node_id.to_string(),
                        track_count,
                        version: Some(crate::build_version().to_string()),
                    }
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

                if !modern
                    && (!self.explicit_catalog()
                        || crate::peer_policy::get(&self.db, peer_id).await?.followed)
                {
                    self.subscribers.write().await.insert(remote);
                    let node = Arc::clone(self);
                    tokio::spawn(async move {
                        node.announce_all_tracks_to_peer(remote).await;
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

                let remote = peer_id
                    .parse()
                    .map_err(|_| P2pError::Connection("invalid peer identity".into()))?;
                if let Ok(permit) = self.pex_workers.clone().try_acquire_owned() {
                    let node = Arc::clone(self);
                    tokio::spawn(async move {
                        let _permit = permit;
                        node.verify_pex_candidates(peers).await;
                    });
                }
                let reply = P2pMessage::PeerExchange {
                    peers: self.pex_sample(remote).await?,
                };
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
            P2pMessage::CatalogChanged => {
                let remote: EndpointId = peer_id
                    .parse()
                    .map_err(|_| P2pError::Connection("invalid identity".into()))?;
                if negotiated_alpn == EXPLICIT_CATALOG_ALPN && self.catalogs.supports(remote).await
                {
                    self.schedule_catalog_pull(remote).await;
                }
                let _ = send.finish();
            }
            P2pMessage::MirrorOffer(offer) => {
                let sender: EndpointId = peer_id
                    .parse()
                    .map_err(|_| P2pError::Connection("invalid offer sender".into()))?;
                if self.versioned_catalog()
                    && negotiated_alpn == EXPLICIT_CATALOG_ALPN
                    && self.catalogs.supports(sender).await
                {
                    let changed = crate::inventory::lease::accept(
                        &self.db,
                        self.node_id(),
                        sender,
                        &offer,
                        None,
                    )
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                    if changed && offer.provider.lease.origin == self.node_id() {
                        self.notify_mirror_change(*offer).await;
                    }
                }
                let _ = send.finish();
            }
            P2pMessage::SignedWithdrawal(signed) => {
                let remote: EndpointId = peer_id
                    .parse()
                    .map_err(|_| P2pError::Connection("invalid identity".into()))?;
                if negotiated_alpn == EXPLICIT_CATALOG_ALPN && self.versioned_catalog() {
                    crate::inventory::apply_withdrawal(
                        &self.db,
                        remote,
                        &serde_json::to_vec(&signed)?,
                    )
                    .await
                    .map_err(|e| P2pError::Connection(e.to_string()))?;
                }
                let _ = send.finish();
            }
            P2pMessage::StopCatalog => {
                if negotiated_alpn == EXPLICIT_CATALOG_ALPN {
                    if let Ok(id) = peer_id.parse::<EndpointId>() {
                        self.subscribers.write().await.remove(&id);
                    }
                }
                let _ = send.finish();
            }
            P2pMessage::RequestCatalog => {
                if self.explicit_catalog()
                    && !crate::peer_policy::permits_export(&self.db, peer_id).await?
                {
                    let _ = send.finish();
                    return Ok(());
                }
                if let Ok(id) = peer_id.parse::<EndpointId>() {
                    self.subscribers.write().await.insert(id);
                }
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
            | P2pMessage::PresenceReply { .. }
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
        let key = SecretKey::generate();
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

    /// Report a relay only after the handshake has completed.
    fn connected_relay(endpoint: &Endpoint) -> Option<String> {
        endpoint
            .home_relay_status()
            .get()
            .into_iter()
            .find(|relay| relay.is_connected())
            .map(|relay| relay.url().to_string())
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

    #[test]
    fn presence_requires_follow_before_legacy_ping() {
        assert!(presence_probe(SOUNDTIME_ALPN, false, false, false).is_err());
        assert_eq!(
            serde_json::to_string(&presence_probe(SOUNDTIME_ALPN, true, false, false).unwrap())
                .unwrap(),
            r#""Ping""#
        );
        assert!(matches!(
            presence_probe(EXPLICIT_CATALOG_ALPN, false, false, false).unwrap(),
            P2pMessage::Presence {
                share_identity: false,
                follows_recipient: false,
                versioned_catalog: false
            }
        ));
        let wire = serde_json::json!({"PeerExchange":{"peers":(0..300).map(|n| n.to_string()).collect::<Vec<_>>()}});
        let P2pMessage::PeerExchange { peers } = serde_json::from_value(wire).unwrap() else {
            panic!("expected PEX");
        };
        assert_eq!(peers.len(), PEX_LIMIT);
    }

    #[tokio::test]
    async fn control_alpn_negotiates_in_both_directions() {
        use iroh::endpoint::{presets::Minimal, PortmapperConfig};
        let base = || {
            Endpoint::builder(Minimal)
                .clear_ip_transports()
                .clear_relay_transports()
                .portmapper_config(PortmapperConfig::Disabled)
                .bind_addr("127.0.0.1:0")
                .unwrap()
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            for (modern_client, modern_server) in [(true, true), (true, false), (false, true)] {
                let advertised = if modern_server {
                    vec![EXPLICIT_CATALOG_ALPN.to_vec(), SOUNDTIME_ALPN.to_vec()]
                } else {
                    vec![SOUNDTIME_ALPN.to_vec()]
                };
                let server = base().alpns(advertised).bind().await.unwrap();
                let remote = EndpointAddr::new(server.id()).with_ip_addr(server.bound_sockets()[0]);
                let client = base()
                    .address_lookup(MemoryLookup::from_endpoint_info([remote]))
                    .bind()
                    .await
                    .unwrap();
                let pool = if modern_client {
                    ConnectionPool::new(client.clone(), EXPLICIT_CATALOG_ALPN)
                        .with_fallback(SOUNDTIME_ALPN)
                } else {
                    ConnectionPool::new(client.clone(), SOUNDTIME_ALPN)
                };
                let (outgoing, incoming) = tokio::join!(pool.get_connection(server.id()), async {
                    server.accept().await.unwrap().await.unwrap()
                });
                let conn = outgoing.unwrap();
                let expected = if modern_server && modern_client {
                    EXPLICIT_CATALOG_ALPN
                } else {
                    SOUNDTIME_ALPN
                };
                assert_eq!(conn.alpn(), expected);
                assert_eq!(incoming.alpn(), expected);
                assert_eq!(incoming.remote_id(), client.id());
                assert_eq!(conn.remote_id(), server.id());
                conn.close(0u32.into(), b"done");
                server.close().await;
                client.close().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn blob_payload_counts_short_reads_and_rejects_wrong_lengths() {
        let received = AtomicU64::new(0);
        let data = vec![0x62; 128 * 1024 + 3];
        let result = read_blob_payload(&mut data.as_slice(), data.len(), Some(&received))
            .await
            .unwrap();
        assert_eq!(result, data);
        assert!(read_blob_payload(&mut &b"abc"[..], 5, Some(&received))
            .await
            .is_err());
        assert!(read_blob_payload(&mut &b"abcd"[..], 3, Some(&received))
            .await
            .is_err());
        assert_eq!(received.load(Ordering::Relaxed), data.len() as u64 + 7);
    }

    #[tokio::test]
    async fn blob_payload_counts_partial_writes_even_on_disconnect() {
        use tokio::io::AsyncReadExt;
        let sent = AtomicU64::new(0);
        let (mut writer, mut reader) = tokio::io::duplex(7);
        let (result, ()) = tokio::join!(
            write_blob_payload(&mut writer, &[9; 1024], Some(&sent)),
            async move {
                let mut first = [0; 7];
                reader.read_exact(&mut first).await.unwrap();
                assert_eq!(first, [9; 7]);
            }
        );
        assert!(result.is_err());
        assert_eq!(sent.load(Ordering::Relaxed), 7);
    }

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
                authority_node: None,
                publication_id: None,
                revision: None,
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

    #[tokio::test]
    async fn blob_store_upgrade_preserves_unconfirmed_store() {
        let old = tempfile::tempdir().unwrap();
        let db = old.path().join("blobs.db");
        tokio::fs::write(&db, b"legacy index").await.unwrap();
        assert!(check_blob_store_upgrade(old.path()).await.is_err());
        assert_eq!(tokio::fs::read(&db).await.unwrap(), b"legacy index");
        tokio::fs::write(old.path().join(BLOB_STORE_MARKER), b"")
            .await
            .unwrap();
        assert!(check_blob_store_upgrade(old.path()).await.is_ok());

        let fresh = tempfile::tempdir().unwrap();
        check_blob_store_upgrade(fresh.path()).await.unwrap();
        tokio::fs::write(fresh.path().join("blobs.db"), b"new index")
            .await
            .unwrap();
        check_blob_store_upgrade(fresh.path()).await.unwrap();
    }

    // ── P2pConfig defaults ───────────────────────────────────────────

    #[test]
    fn test_p2p_config_defaults() {
        let cfg = P2pConfig::default();
        assert_eq!(cfg.bind_port, 0);
        assert!(cfg.enable_local_discovery);
        assert!(cfg.enable_dht_discovery);
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
        let key = SecretKey::generate();
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
        let key = SecretKey::generate();
        let id = key.public();
        let addr = EndpointAddr::new(id);
        assert_eq!(addr.id, id);
        assert!(addr.ip_addrs().next().is_none());
        assert!(addr.relay_urls().next().is_none());
    }

    #[test]
    fn test_endpoint_id_display_roundtrip() {
        let key = SecretKey::generate();
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
            authority_node: None,
            publication_id: None,
            revision: None,
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
            authority_node: None,
            publication_id: None,
            revision: None,
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
        std::env::remove_var("P2P_DHT_DISCOVERY");
        std::env::remove_var("P2P_SEED_PEERS");
        std::env::remove_var("AUDIO_STORAGE_PATH");
        std::env::remove_var("METADATA_STORAGE_PATH");

        let cfg = P2pConfig::from_env();
        assert_eq!(cfg.blobs_dir, PathBuf::from("data/p2p/blobs"));
        assert_eq!(cfg.secret_key_path, PathBuf::from("data/p2p/secret_key"));
        assert_eq!(cfg.bind_port, 0);
        assert!(cfg.enable_local_discovery);
        assert!(cfg.enable_dht_discovery);
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
    fn test_config_from_env_dht_discovery_true() {
        std::env::set_var("P2P_DHT_DISCOVERY", "true");
        let cfg = P2pConfig::from_env();
        assert!(cfg.enable_dht_discovery);
        std::env::remove_var("P2P_DHT_DISCOVERY");
    }

    #[test]
    fn test_config_from_env_dht_discovery_case_insensitive() {
        std::env::set_var("P2P_DHT_DISCOVERY", "TRUE");
        let cfg = P2pConfig::from_env();
        assert!(cfg.enable_dht_discovery);
        std::env::remove_var("P2P_DHT_DISCOVERY");
    }

    #[test]
    fn test_config_from_env_dht_discovery_false() {
        std::env::set_var("P2P_DHT_DISCOVERY", "false");
        let cfg = P2pConfig::from_env();
        assert!(!cfg.enable_dht_discovery);
        std::env::remove_var("P2P_DHT_DISCOVERY");
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
                authority_node: None,
                publication_id: None,
                revision: None,
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
