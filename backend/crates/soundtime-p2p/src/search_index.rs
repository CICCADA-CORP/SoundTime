//! Bloom filter-based search index for P2P content discovery.
//!
//! Each node maintains a Bloom filter of searchable terms (track titles,
//! artist names, album titles). Peers exchange these compact filters so
//! a node can determine which peers *might* have results for a given query
//! without downloading their entire catalogs.
//!
//! A Bloom filter of 1M entries takes ~1.2 MB with 1% false positive rate.
//! This allows efficient search routing: instead of broadcasting a search
//! query to every peer, we only query peers whose Bloom filter matches.

use bloomfilter::Bloom;
use sea_orm::{DatabaseConnection, EntityTrait, PaginatorTrait};
use serde::{Deserialize, Serialize};
use soundtime_db::entities::{album, artist, track};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Default Bloom filter capacity — number of expected items.
const DEFAULT_BLOOM_CAPACITY: usize = 100_000;
/// Target false positive rate (1%).
const FALSE_POSITIVE_RATE: f64 = 0.01;
/// Page size for paginated database queries during rebuild.
const REBUILD_PAGE_SIZE: u64 = 1000;

/// Compact serializable representation of a Bloom filter for network exchange.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BloomFilterData {
    /// The raw bitmap data (base64 or raw bytes serialized via serde)
    pub bitmap: Vec<u8>,
    /// Number of hash functions used
    pub num_hashes: u32,
    /// Bitmap size in bits
    pub bitmap_bits: u64,
    /// SIP hash keys used by the Bloom filter — needed to reconstruct it
    pub sip_keys: [(u64, u64); 2],
    /// Number of items inserted
    pub item_count: u64,
}

/// Per-peer search index — stores the peer's Bloom filter for query routing.
#[derive(Clone, Debug)]
pub struct PeerSearchIndex {
    pub node_id: String,
    pub bloom: BloomFilterData,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// Manages local and peer Bloom filter indexes.
pub struct SearchIndex {
    /// Our local Bloom filter of searchable terms
    local_bloom: RwLock<Bloom<String>>,
    /// Number of items in local bloom
    local_item_count: RwLock<u64>,
    /// Bloom filters received from peers, keyed by NodeId
    peer_indexes: RwLock<HashMap<String, PeerSearchIndex>>,
    /// Flag indicating the Bloom filter needs a full rebuild (e.g. after a track deletion).
    /// Bloom filters don't support removal, so deletions require a complete rebuild.
    dirty: AtomicBool,
}

impl SearchIndex {
    /// Create a new, empty search index.
    pub fn new() -> Self {
        Self {
            local_bloom: RwLock::new(Bloom::new_for_fp_rate(
                DEFAULT_BLOOM_CAPACITY,
                FALSE_POSITIVE_RATE,
            )),
            local_item_count: RwLock::new(0),
            peer_indexes: RwLock::new(HashMap::new()),
            dirty: AtomicBool::new(false),
        }
    }

    /// Normalize a term for insertion / lookup: lowercase + split words.
    fn normalize_terms(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() >= 2) // skip very short words
            .map(|w| w.to_string())
            .collect()
    }

    /// Insert searchable terms for a track into the local Bloom filter.
    /// Call this when a track is added locally or replicated from a peer.
    pub async fn insert_track(&self, title: &str, artist_name: &str, album_title: Option<&str>) {
        let mut bloom = self.local_bloom.write().await;
        let mut count = self.local_item_count.write().await;

        for term in Self::normalize_terms(title) {
            bloom.set(&term);
            *count += 1;
        }
        for term in Self::normalize_terms(artist_name) {
            bloom.set(&term);
            *count += 1;
        }
        if let Some(album) = album_title {
            for term in Self::normalize_terms(album) {
                bloom.set(&term);
                *count += 1;
            }
        }
    }

    /// Check if a query term *might* match our local index.
    pub async fn local_might_match(&self, query: &str) -> bool {
        let bloom = self.local_bloom.read().await;
        // All terms in the query must be present (AND semantics)
        Self::normalize_terms(query)
            .iter()
            .all(|term| bloom.check(term))
    }

    /// Export the local Bloom filter as a compact serializable structure.
    pub async fn export_local_bloom(&self) -> BloomFilterData {
        let bloom = self.local_bloom.read().await;
        let count = self.local_item_count.read().await;
        let bitmap = bloom.bitmap();
        let num_hashes = bloom.number_of_hash_functions();
        let bitmap_bits = bloom.number_of_bits();

        let sip_keys = bloom.sip_keys();

        BloomFilterData {
            bitmap,
            num_hashes,
            bitmap_bits,
            sip_keys,
            item_count: *count,
        }
    }

    /// Import a peer's Bloom filter for search routing.
    pub async fn import_peer_bloom(&self, node_id: &str, bloom_data: BloomFilterData) {
        let peer_index = PeerSearchIndex {
            node_id: node_id.to_string(),
            bloom: bloom_data,
            last_updated: chrono::Utc::now(),
        };
        let mut indexes = self.peer_indexes.write().await;
        indexes.insert(node_id.to_string(), peer_index);
        debug!(%node_id, "imported peer bloom filter");
    }

    /// Check if a peer's Bloom filter might contain results for a query.
    pub async fn peer_might_match(&self, node_id: &str, query: &str) -> bool {
        let indexes = self.peer_indexes.read().await;
        let Some(peer_index) = indexes.get(node_id) else {
            // No bloom filter from this peer — assume it might have results
            return true;
        };

        // Reconstruct a Bloom from the raw data to check
        let bloom = Bloom::<String>::from_existing(
            &peer_index.bloom.bitmap,
            peer_index.bloom.bitmap_bits,
            peer_index.bloom.num_hashes,
            peer_index.bloom.sip_keys,
        );

        Self::normalize_terms(query)
            .iter()
            .all(|term| bloom.check(term))
    }

    /// Get the NodeIds of peers that might have results for a query.
    /// Returns peers whose Bloom filter matches the query terms.
    pub async fn peers_matching_query(&self, query: &str) -> Vec<String> {
        let indexes = self.peer_indexes.read().await;
        let terms = Self::normalize_terms(query);
        if terms.is_empty() {
            return indexes.keys().cloned().collect();
        }

        let mut matching = Vec::new();
        for (node_id, peer_index) in indexes.iter() {
            let bloom = Bloom::<String>::from_existing(
                &peer_index.bloom.bitmap,
                peer_index.bloom.bitmap_bits,
                peer_index.bloom.num_hashes,
                peer_index.bloom.sip_keys,
            );

            if terms.iter().all(|term| bloom.check(term)) {
                matching.push(node_id.clone());
            }
        }

        info!(
            query = query,
            total_peers = indexes.len(),
            matching_peers = matching.len(),
            "bloom filter routing"
        );

        matching
    }

    /// Remove a peer's index (when peer is removed from registry).
    pub async fn remove_peer(&self, node_id: &str) {
        let mut indexes = self.peer_indexes.write().await;
        indexes.remove(node_id);
    }

    /// Get count of indexed peers.
    pub async fn indexed_peer_count(&self) -> usize {
        let indexes = self.peer_indexes.read().await;
        indexes.len()
    }

    /// Rebuild the local Bloom filter from a full list of tracks.
    /// Called at startup to populate the index from the database.
    pub async fn rebuild_from_tracks(
        &self,
        tracks: &[(String, String, Option<String>)], // (title, artist_name, album_title)
    ) {
        let mut bloom = self.local_bloom.write().await;
        let mut count = self.local_item_count.write().await;

        // Reset
        *bloom = Bloom::new_for_fp_rate(
            tracks.len().max(DEFAULT_BLOOM_CAPACITY),
            FALSE_POSITIVE_RATE,
        );
        *count = 0;

        for (title, artist, album) in tracks {
            for term in Self::normalize_terms(title) {
                bloom.set(&term);
                *count += 1;
            }
            for term in Self::normalize_terms(artist) {
                bloom.set(&term);
                *count += 1;
            }
            if let Some(alb) = album {
                for term in Self::normalize_terms(alb) {
                    bloom.set(&term);
                    *count += 1;
                }
            }
        }

        info!(
            tracks = tracks.len(),
            terms = *count,
            "rebuilt local bloom filter"
        );
    }

    /// Rebuild the local Bloom filter from the database using paginated queries.
    ///
    /// Instead of loading all tracks into memory at once, this fetches pages of
    /// `REBUILD_PAGE_SIZE` records at a time to avoid OOM on large instances.
    /// Resets the dirty flag on success.
    pub async fn rebuild_from_db(&self, db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
        // First, count total tracks to size the Bloom filter appropriately.
        let total_tracks = track::Entity::find().count(db).await?;

        let mut bloom = self.local_bloom.write().await;
        let mut count = self.local_item_count.write().await;

        // Reset the Bloom filter with capacity based on actual track count.
        *bloom = Bloom::new_for_fp_rate(
            (total_tracks as usize).max(DEFAULT_BLOOM_CAPACITY),
            FALSE_POSITIVE_RATE,
        );
        *count = 0;

        let num_pages = if total_tracks == 0 {
            0
        } else {
            total_tracks.div_ceil(REBUILD_PAGE_SIZE)
        };

        for page_num in 0..num_pages {
            let tracks_with_artists: Vec<(track::Model, Option<artist::Model>)> =
                track::Entity::find()
                    .find_also_related(artist::Entity)
                    .paginate(db, REBUILD_PAGE_SIZE)
                    .fetch_page(page_num)
                    .await?;

            for (t, artist_opt) in &tracks_with_artists {
                let artist_name = artist_opt
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_default();

                // Fetch album title if available (individual query per track with album).
                let album_title = if let Some(album_id) = t.album_id {
                    album::Entity::find_by_id(album_id)
                        .one(db)
                        .await
                        .ok()
                        .flatten()
                        .map(|a| a.title)
                } else {
                    None
                };

                for term in Self::normalize_terms(&t.title) {
                    bloom.set(&term);
                    *count += 1;
                }
                for term in Self::normalize_terms(&artist_name) {
                    bloom.set(&term);
                    *count += 1;
                }
                if let Some(ref alb) = album_title {
                    for term in Self::normalize_terms(alb) {
                        bloom.set(&term);
                        *count += 1;
                    }
                }
            }

            debug!(
                page = page_num,
                tracks_in_page = tracks_with_artists.len(),
                "processed search index page"
            );
        }

        // Clear the dirty flag after a successful full rebuild.
        self.dirty.store(false, Ordering::Release);

        info!(
            tracks = total_tracks,
            terms = *count,
            "rebuilt local bloom filter from database (paginated)"
        );

        Ok(())
    }

    /// Add a single track's tokens to the search index without a full rebuild.
    ///
    /// Called immediately when a new track is added to the local catalog so it
    /// becomes visible to peers without waiting for the next exchange cycle.
    pub async fn add_track_tokens(&self, title: &str, artist: &str, album: Option<&str>) {
        let mut bloom = self.local_bloom.write().await;
        let mut count = self.local_item_count.write().await;

        for term in Self::normalize_terms(title) {
            bloom.set(&term);
            *count += 1;
        }
        for term in Self::normalize_terms(artist) {
            bloom.set(&term);
            *count += 1;
        }
        if let Some(alb) = album {
            for term in Self::normalize_terms(alb) {
                bloom.set(&term);
                *count += 1;
            }
        }

        debug!(
            title = title,
            artist = artist,
            "added track tokens to search index"
        );
    }

    /// Mark the search index as dirty, requiring a full rebuild.
    ///
    /// Bloom filters don't support removal, so when a track is deleted this
    /// flag is set. The next exchange cycle (or an explicit rebuild) should
    /// call `rebuild_from_db` to create a clean Bloom filter.
    pub async fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
        info!("search index marked dirty — full rebuild needed");
    }

    /// Check whether the index has been marked dirty and needs a full rebuild.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_match() {
        let idx = SearchIndex::new();
        idx.insert_track("Bohemian Rhapsody", "Queen", Some("A Night at the Opera"))
            .await;

        assert!(idx.local_might_match("bohemian").await);
        assert!(idx.local_might_match("queen").await);
        assert!(idx.local_might_match("opera").await);
        assert!(idx.local_might_match("bohemian rhapsody").await);
    }

    #[tokio::test]
    async fn test_no_false_match_on_empty() {
        let idx = SearchIndex::new();
        // Empty bloom should not match anything
        assert!(!idx.local_might_match("nonexistent").await);
    }

    #[tokio::test]
    async fn test_export_import_roundtrip() {
        let idx = SearchIndex::new();
        idx.insert_track("Test Track", "Test Artist", Some("Test Album"))
            .await;

        let exported = idx.export_local_bloom().await;

        // Import into a second index as a "peer"
        let idx2 = SearchIndex::new();
        idx2.import_peer_bloom("peer1", exported).await;

        assert!(idx2.peer_might_match("peer1", "test").await);
        assert!(idx2.peer_might_match("peer1", "track").await);
        assert!(idx2.peer_might_match("peer1", "artist").await);
    }

    #[tokio::test]
    async fn test_peers_matching_query() {
        let idx = SearchIndex::new();

        // Simulate peer1 with jazz music
        let jazz_idx = SearchIndex::new();
        jazz_idx
            .insert_track("Take Five", "Dave Brubeck", Some("Time Out"))
            .await;
        let jazz_bloom = jazz_idx.export_local_bloom().await;
        idx.import_peer_bloom("peer1", jazz_bloom).await;

        // Simulate peer2 with rock music
        let rock_idx = SearchIndex::new();
        rock_idx
            .insert_track("Stairway to Heaven", "Led Zeppelin", None)
            .await;
        let rock_bloom = rock_idx.export_local_bloom().await;
        idx.import_peer_bloom("peer2", rock_bloom).await;

        // Query for jazz should only match peer1
        let matches = idx.peers_matching_query("brubeck").await;
        assert!(matches.contains(&"peer1".to_string()));
        assert!(!matches.contains(&"peer2".to_string()));

        // Query for rock should only match peer2
        let matches = idx.peers_matching_query("zeppelin").await;
        assert!(!matches.contains(&"peer1".to_string()));
        assert!(matches.contains(&"peer2".to_string()));
    }

    #[tokio::test]
    async fn test_rebuild_from_tracks() {
        let idx = SearchIndex::new();
        let tracks = vec![
            (
                "Song A".to_string(),
                "Artist X".to_string(),
                Some("Album 1".to_string()),
            ),
            ("Song B".to_string(), "Artist Y".to_string(), None),
        ];
        idx.rebuild_from_tracks(&tracks).await;
        assert!(idx.local_might_match("song").await);
        assert!(idx.local_might_match("artist").await);
        assert!(idx.local_might_match("album").await);
    }

    // ── normalize_terms edge cases ───────────────────────────────────

    #[test]
    fn test_normalize_terms_basic() {
        let terms = SearchIndex::normalize_terms("Hello World");
        assert_eq!(terms, vec!["hello", "world"]);
    }

    #[test]
    fn test_normalize_terms_short_words_filtered() {
        // Words with < 2 characters should be filtered out
        let terms = SearchIndex::normalize_terms("I am a DJ");
        // "i" and "a" are 1 char → filtered; "am" (2 chars) kept, "dj" kept
        assert_eq!(terms, vec!["am", "dj"]);
    }

    #[test]
    fn test_normalize_terms_empty_string() {
        let terms = SearchIndex::normalize_terms("");
        assert!(terms.is_empty());
    }

    #[test]
    fn test_normalize_terms_only_short_words() {
        let terms = SearchIndex::normalize_terms("I a");
        assert!(terms.is_empty());
    }

    #[test]
    fn test_normalize_terms_unicode() {
        let terms = SearchIndex::normalize_terms("Étoile café");
        assert_eq!(terms, vec!["étoile", "café"]);
    }

    #[test]
    fn test_normalize_terms_extra_whitespace() {
        let terms = SearchIndex::normalize_terms("  hello   world  ");
        assert_eq!(terms, vec!["hello", "world"]);
    }

    #[test]
    fn test_normalize_terms_mixed_case() {
        let terms = SearchIndex::normalize_terms("Bohemian RHAPSODY");
        assert_eq!(terms, vec!["bohemian", "rhapsody"]);
    }

    // ── local_might_match edge cases ─────────────────────────────────

    #[tokio::test]
    async fn test_local_might_match_empty_query() {
        let idx = SearchIndex::new();
        idx.insert_track("Something", "Someone", None).await;
        // Empty query → normalize_terms is empty → `.all()` on empty iter returns true
        assert!(idx.local_might_match("").await);
    }

    #[tokio::test]
    async fn test_local_might_match_and_semantics() {
        let idx = SearchIndex::new();
        idx.insert_track("Bohemian Rhapsody", "Queen", None).await;
        // Both terms present → true
        assert!(idx.local_might_match("bohemian queen").await);
        // One term missing → false (AND semantics)
        assert!(!idx.local_might_match("bohemian metallica").await);
    }

    #[tokio::test]
    async fn test_local_might_match_case_insensitive() {
        let idx = SearchIndex::new();
        idx.insert_track("LOUD TRACK", "ARTIST", None).await;
        assert!(idx.local_might_match("loud").await);
        assert!(idx.local_might_match("LOUD").await);
        assert!(idx.local_might_match("Loud").await);
    }

    // ── export_local_bloom structure ─────────────────────────────────

    #[tokio::test]
    async fn test_export_bloom_structure() {
        let idx = SearchIndex::new();
        idx.insert_track("Test", "Artist", Some("Album")).await;
        let bloom = idx.export_local_bloom().await;

        assert!(!bloom.bitmap.is_empty());
        assert!(bloom.num_hashes > 0);
        assert!(bloom.bitmap_bits > 0);
        assert!(bloom.item_count > 0);
        // SIP keys should be set
        assert!(bloom.sip_keys[0] != (0, 0) || bloom.sip_keys[1] != (0, 0));
    }

    #[tokio::test]
    async fn test_export_bloom_empty_index() {
        let idx = SearchIndex::new();
        let bloom = idx.export_local_bloom().await;
        assert_eq!(bloom.item_count, 0);
        assert!(!bloom.bitmap.is_empty()); // bitmap exists even when empty
    }

    // ── peer_might_match edge cases ──────────────────────────────────

    #[tokio::test]
    async fn test_peer_might_match_unknown_peer() {
        let idx = SearchIndex::new();
        // No bloom imported → should return true (assume peer might have results)
        assert!(idx.peer_might_match("unknown_peer", "anything").await);
    }

    #[tokio::test]
    async fn test_peer_might_match_no_match() {
        let idx = SearchIndex::new();

        let jazz = SearchIndex::new();
        jazz.insert_track("Take Five", "Dave Brubeck", None).await;
        let bloom = jazz.export_local_bloom().await;
        idx.import_peer_bloom("jazz_peer", bloom).await;

        assert!(!idx.peer_might_match("jazz_peer", "metallica").await);
    }

    // ── peers_matching_query edge cases ──────────────────────────────

    #[tokio::test]
    async fn test_peers_matching_query_empty_query() {
        let idx = SearchIndex::new();

        let peer = SearchIndex::new();
        peer.insert_track("Track", "Artist", None).await;
        let bloom = peer.export_local_bloom().await;
        idx.import_peer_bloom("peer1", bloom).await;

        // Empty query → returns all peers
        let matches = idx.peers_matching_query("").await;
        assert_eq!(matches.len(), 1);
        assert!(matches.contains(&"peer1".to_string()));
    }

    #[tokio::test]
    async fn test_peers_matching_query_no_peers() {
        let idx = SearchIndex::new();
        let matches = idx.peers_matching_query("anything").await;
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn test_peers_matching_query_all_match() {
        let idx = SearchIndex::new();

        // Both peers have "music" in their index
        for peer_id in &["peer1", "peer2"] {
            let peer = SearchIndex::new();
            peer.insert_track("Music Track", "Artist", None).await;
            let bloom = peer.export_local_bloom().await;
            idx.import_peer_bloom(peer_id, bloom).await;
        }

        let matches = idx.peers_matching_query("music").await;
        assert_eq!(matches.len(), 2);
    }

    // ── remove_peer ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_remove_peer() {
        let idx = SearchIndex::new();
        let peer = SearchIndex::new();
        peer.insert_track("Track", "Artist", None).await;
        let bloom = peer.export_local_bloom().await;
        idx.import_peer_bloom("peer1", bloom).await;

        assert_eq!(idx.indexed_peer_count().await, 1);
        idx.remove_peer("peer1").await;
        assert_eq!(idx.indexed_peer_count().await, 0);
    }

    #[tokio::test]
    async fn test_remove_peer_nonexistent() {
        let idx = SearchIndex::new();
        // Should not panic
        idx.remove_peer("ghost").await;
        assert_eq!(idx.indexed_peer_count().await, 0);
    }

    // ── indexed_peer_count ───────────────────────────────────────────

    #[tokio::test]
    async fn test_indexed_peer_count() {
        let idx = SearchIndex::new();
        assert_eq!(idx.indexed_peer_count().await, 0);

        for i in 0..5 {
            let peer = SearchIndex::new();
            peer.insert_track(&format!("Track {i}"), "Artist", None)
                .await;
            idx.import_peer_bloom(&format!("peer{i}"), peer.export_local_bloom().await)
                .await;
        }
        assert_eq!(idx.indexed_peer_count().await, 5);
    }

    // ── rebuild_from_tracks edge cases ───────────────────────────────

    #[tokio::test]
    async fn test_rebuild_from_empty_tracks() {
        let idx = SearchIndex::new();
        // Insert something first
        idx.insert_track("Old Track", "Old Artist", None).await;
        assert!(idx.local_might_match("old").await);

        // Rebuild with empty list — should clear the index
        idx.rebuild_from_tracks(&[]).await;
        assert!(!idx.local_might_match("old").await);
    }

    #[tokio::test]
    async fn test_rebuild_replaces_previous() {
        let idx = SearchIndex::new();
        idx.insert_track("First", "First Artist", None).await;
        assert!(idx.local_might_match("first").await);

        // Rebuild with different tracks
        let tracks = vec![("Second".to_string(), "Second Artist".to_string(), None)];
        idx.rebuild_from_tracks(&tracks).await;
        // Old track should no longer match (bloom was reset)
        // Note: due to bloom filter false positives, "first" MIGHT still match
        // but "second" should definitely match
        assert!(idx.local_might_match("second").await);
    }

    // ── BloomFilterData serde roundtrip ──────────────────────────────

    #[test]
    fn test_bloom_filter_data_serde() {
        let data = BloomFilterData {
            bitmap: vec![1, 2, 3, 4, 5],
            num_hashes: 10,
            bitmap_bits: 8192,
            sip_keys: [(111, 222), (333, 444)],
            item_count: 99,
        };
        let json = serde_json::to_string(&data).unwrap();
        let decoded: BloomFilterData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.bitmap, vec![1, 2, 3, 4, 5]);
        assert_eq!(decoded.num_hashes, 10);
        assert_eq!(decoded.bitmap_bits, 8192);
        assert_eq!(decoded.sip_keys, [(111, 222), (333, 444)]);
        assert_eq!(decoded.item_count, 99);
    }

    #[test]
    fn test_bloom_filter_data_clone() {
        let data = BloomFilterData {
            bitmap: vec![0xFF; 64],
            num_hashes: 7,
            bitmap_bits: 512,
            sip_keys: [(1, 2), (3, 4)],
            item_count: 50,
        };
        let cloned = data.clone();
        assert_eq!(data.bitmap, cloned.bitmap);
        assert_eq!(data.num_hashes, cloned.num_hashes);
    }

    // ── Default impl ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_search_index_default() {
        let idx = SearchIndex::default();
        assert_eq!(idx.indexed_peer_count().await, 0);
        assert!(!idx.local_might_match("anything").await);
    }

    // ── Insert multiple tracks and verify ────────────────────────────

    #[tokio::test]
    async fn test_insert_multiple_tracks() {
        let idx = SearchIndex::new();
        idx.insert_track("Bohemian Rhapsody", "Queen", Some("A Night at the Opera"))
            .await;
        idx.insert_track(
            "Stairway to Heaven",
            "Led Zeppelin",
            Some("Led Zeppelin IV"),
        )
        .await;
        idx.insert_track("Hotel California", "Eagles", Some("Hotel California"))
            .await;

        assert!(idx.local_might_match("bohemian").await);
        assert!(idx.local_might_match("stairway").await);
        assert!(idx.local_might_match("hotel").await);
        assert!(idx.local_might_match("queen").await);
        assert!(idx.local_might_match("zeppelin").await);
        assert!(idx.local_might_match("eagles").await);
    }

    // ── Import replaces existing peer bloom ──────────────────────────

    #[tokio::test]
    async fn test_import_peer_bloom_replaces() {
        let idx = SearchIndex::new();

        // First import with jazz
        let jazz = SearchIndex::new();
        jazz.insert_track("Take Five", "Dave Brubeck", None).await;
        idx.import_peer_bloom("peer1", jazz.export_local_bloom().await)
            .await;
        assert!(idx.peer_might_match("peer1", "brubeck").await);

        // Replace with rock
        let rock = SearchIndex::new();
        rock.insert_track("Thunderstruck", "AC DC", None).await;
        idx.import_peer_bloom("peer1", rock.export_local_bloom().await)
            .await;

        // Should now match rock, not jazz
        assert!(idx.peer_might_match("peer1", "thunderstruck").await);
        // Peer count should still be 1 (replaced, not added)
        assert_eq!(idx.indexed_peer_count().await, 1);
    }

    // ── PeerSearchIndex debug ────────────────────────────────────────

    #[test]
    fn test_peer_search_index_debug() {
        let psi = PeerSearchIndex {
            node_id: "test_node".to_string(),
            bloom: BloomFilterData {
                bitmap: vec![],
                num_hashes: 0,
                bitmap_bits: 0,
                sip_keys: [(0, 0), (0, 0)],
                item_count: 0,
            },
            last_updated: chrono::Utc::now(),
        };
        let debug = format!("{:?}", psi);
        assert!(debug.contains("PeerSearchIndex"));
        assert!(debug.contains("test_node"));
    }

    // ── add_track_tokens ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_track_tokens_basic() {
        let idx = SearchIndex::new();
        idx.add_track_tokens("Bohemian Rhapsody", "Queen", Some("A Night at the Opera"))
            .await;

        assert!(idx.local_might_match("bohemian").await);
        assert!(idx.local_might_match("queen").await);
        assert!(idx.local_might_match("opera").await);
    }

    #[tokio::test]
    async fn test_add_track_tokens_no_album() {
        let idx = SearchIndex::new();
        idx.add_track_tokens("Take Five", "Dave Brubeck", None)
            .await;

        assert!(idx.local_might_match("five").await);
        assert!(idx.local_might_match("brubeck").await);
    }

    #[tokio::test]
    async fn test_add_track_tokens_incremental() {
        let idx = SearchIndex::new();
        idx.add_track_tokens("Song A", "Artist X", None).await;
        idx.add_track_tokens("Song B", "Artist Y", None).await;

        // Both tracks should be searchable
        assert!(idx.local_might_match("song").await);
        assert!(idx.local_might_match("artist").await);
    }

    #[tokio::test]
    async fn test_add_track_tokens_updates_count() {
        let idx = SearchIndex::new();
        let before = *idx.local_item_count.read().await;
        idx.add_track_tokens("Hello World", "Test Artist", None)
            .await;
        let after = *idx.local_item_count.read().await;
        assert!(after > before);
    }

    // ── mark_dirty / is_dirty ─────────────────────────────────────────

    #[tokio::test]
    async fn test_mark_dirty_and_is_dirty() {
        let idx = SearchIndex::new();
        // New index is not dirty
        assert!(!idx.is_dirty());

        idx.mark_dirty().await;
        assert!(idx.is_dirty());
    }

    #[tokio::test]
    async fn test_dirty_cleared_by_rebuild_from_tracks() {
        let idx = SearchIndex::new();
        idx.mark_dirty().await;
        assert!(idx.is_dirty());

        // rebuild_from_tracks does NOT clear the dirty flag (only rebuild_from_db does)
        let tracks = vec![("Song".to_string(), "Artist".to_string(), None)];
        idx.rebuild_from_tracks(&tracks).await;
        // dirty flag is still set because rebuild_from_tracks doesn't clear it
        assert!(idx.is_dirty());
    }

    #[test]
    fn test_is_dirty_default_false() {
        let idx = SearchIndex::new();
        assert!(!idx.is_dirty());
    }
}
