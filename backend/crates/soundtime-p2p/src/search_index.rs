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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Default Bloom filter capacity — number of expected items.
const DEFAULT_BLOOM_CAPACITY: usize = 100_000;
/// Target false positive rate (1%).
const FALSE_POSITIVE_RATE: f64 = 0.01;

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
}
