# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2026-03-10]

### Added

- **Autoplay / Auto-queue** — Continuous playback that automatically queues similar tracks when the queue empties.
  - Toggle in both mini and expanded player UI with persistent state via `localStorage`.
  - Uses the radio API (`/api/radio/next`) with `similar` seed type for intelligent track selection.
  - Excludes recently played tracks (up to 2000) to avoid repetition.
  - Svelte 5 rune store (`queue.svelte.ts`) with cross-store event communication via `CustomEvent`.
  - i18n support in all 5 languages (EN, FR, ES, RU, ZH).
  - 19 unit tests covering queue store, AudioPlayer, and ExpandedPlayer.

- **P2P DHT Discovery** — Mainline DHT peer discovery via the Pkarr protocol.
  - Uses iroh's `DhtAddressLookup` (feature flag `address-lookup-pkarr-dht`) for global peer discovery without relying solely on DNS/relay infrastructure.
  - Enabled by default (`P2P_DHT_DISCOVERY=true`); configurable via environment variable.
  - Read-only DHT status indicator in the admin P2P dashboard (Discovery section).
  - `P2pStatus.dht_discovery_enabled` field exposed via `GET /api/p2p/status`.
  - i18n support in all 5 languages.
  - 3 new backend tests (env parsing: default true, explicit true, case-insensitive false).

- **Database Migration #33** — Collation version refresh (`ALTER DATABASE ... REFRESH COLLATION VERSION`) for compatibility with the pgvector Docker image switch.

- **P2P Track Health Monitoring** (`soundtime-p2p/track_health`)
  - `TrackHealthManager` with per-track health state tracking (Healthy, Recovered, Degraded, Dereferenced).
  - Auto-retry on playback failure via `auto_repair_on_failure` — fetches from origin peer, then tries alternative peers.
  - 3-strike dereference: tracks that fail 3 consecutive recovery attempts are marked unavailable.
  - Automatic re-referencing: dereferenced tracks are restored when their blob becomes available locally or recovery succeeds.
  - Periodic background health sweep (`spawn_health_monitor`, `run_health_sweep`) — configurable interval and batch size.
  - Duplicate resolution with `select_best_copy` — ranks copies by format quality, bitrate, sample rate, and peer online status.
  - Quality scoring: format ranking (FLAC > WAV > OPUS > OGG > AAC > MP3), bitrate/sample rate bonuses.
  - Semaphore-based concurrency (32 simultaneous recoveries max) to prevent resource exhaustion.
  - `TrackFetcher` async trait for testable I/O and mock-based testing.
  - `persist_track_status` for database sync of health state changes.
  - 237+ unit tests with >90% code coverage.

- **Distributed Search** (`soundtime-p2p/search_index`)
  - Bloom filter-based search index (~1.2 MB per peer, 1% false positive rate).
  - Smart query routing: only peers whose Bloom filter matches the query receive the search request.
  - Term normalization (lowercase, word splitting, short word filtering).
  - `BloomFilterData` serialization for efficient peer-to-peer exchange.
  - `SearchQuery` / `SearchResults` P2P messages in protocol.

- **Incremental Catalog Sync**
  - `CatalogDelta` message type for sending only new tracks to peers.
  - `incremental_sync_to_peer` avoids redundant full catalog pushes.

- **Public Instance Listing** (`soundtime-server/listing_worker`)
  - Periodic heartbeat (every 5 min) to the SoundTime public directory.
  - Configurable via admin settings (`listing_public`, `listing_url`, `listing_domain`).
  - Admin API endpoint `POST /api/admin/listing/trigger` for immediate heartbeat.
  - Local domain detection to prevent invalid registrations.

- **MusicBrainz Integration** (`soundtime-p2p/musicbrainz`)
  - Metadata enrichment via MusicBrainz API v2.
  - Rate-limited client (1 req/sec) with confidence scoring (threshold ≥ 80).

- **Peer Exchange (PEX)**
  - Automatic periodic exchange of known peer lists (every 5 min).
  - Gossip-style mesh for organic peer discovery.

### Changed

- **iroh upgraded from 0.32 to 0.96** — includes iroh-blobs 0.96 with `FsStore`.
- **Rust minimum version raised to 1.93** (Docker image: `rust:1.93-slim`).
- **DHT discovery enabled by default** — `P2P_DHT_DISCOVERY` defaults to `true` (opt-out rather than opt-in).
- P2P identity types aliased: `NodeId → EndpointId`, `NodeAddr → EndpointAddr` for backward compatibility.
- `process_health_batch` now checks dereferenced tracks for local blob availability instead of skipping them permanently.
- `auto_repair_on_failure` attempts recovery for dereferenced tracks instead of short-circuiting.

### Fixed

- **AlbumCard lazy-fetch** (Issue #3) — Album track count and duration now load lazily from the API instead of requiring upfront data, preventing missing stats on paginated views.
- **Admin stats storage calculation** (Issue #4) — Fixed `SUM(file_size)` overflow by casting to `::bigint` in the admin stats SQL query.
- **CI pipeline** — Fixed `cargo fmt` and `clippy` failures across multiple crates; fixed Vitest mock hoisting with `vi.hoisted()`.
- **CodeQL security alerts** — Resolved cleartext logging (#4), tightened GitHub Actions workflow permissions (#34, #35, #36).
- Dead code warning for `clean_domain` in listing worker (now `#[cfg(test)]`).

### Security

- Updated npm dependencies to resolve known vulnerabilities (12 of 15 Dependabot alerts resolved; 3 accepted as temporary risk in wasmtime/extism transitive dependencies).
- Tightened GitHub Actions CI workflow permissions from default to explicit read-only (`contents: read`).
- Removed cleartext credential logging from editorial playlist handler.

## [1.0.0] - 2026-02-01

**Initial Public Release** of SoundTime, the sovereign audio streaming platform.

### Added

- **Core Platform**
  - Full-stack architecture with Rust (Axum) backend and SvelteKit frontend.
  - PostgreSQL database integration via Sea-ORM.
  - High-performance audio decoding with Symphonia.
  - Metadata extraction support for MP3, FLAC, M4A, OGG via Lofty.

- **Frontend / UI**
  - Modern, responsive SPA built with Svelte 5 and TailwindCSS.
  - "Shadcn-svelte" UI component library integration.
  - Persistent audio player with queue management (pinia-style stores).
  - Waveform visualization for tracks.

- **Networking & P2P**
  - **Iroh Integration**: Direct P2P syncing between devices (replacing legacy ActivityPub code).
  - Encrypted node-to-node communication.
  - Automatic peer discovery on local networks.

- **Authentication**
  - Secure signup/login flow with Argon2id password hashing.
  - JWT-based session management (Access + Refresh tokens).
  - Rate limiting on sensitive endpoints.

- **Deployment**
  - Docker Compose setup for easy self-hosting.
  - Nginx reverse proxy configuration.
  - Environment variable configuration via `.env` files.

### Security
- Implemented robust Content Security Policy (CSP).
- Added `tower-governor` for DOS protection/rate-limiting.
- Secure HTTP headers hardened for production.

---

*SoundTime is a product of CICCADA CORP.*
