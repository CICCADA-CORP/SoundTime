# P2P Networking

SoundTime uses [iroh](https://iroh.computer/) by [n0.computer](https://n0.computer/) for peer-to-peer music sharing between instances. This document covers the protocol design, peer discovery, catalog synchronization, and configuration.

## Overview

Each SoundTime instance runs an **iroh node** that communicates over **QUIC** (UDP). Instances discover each other, exchange track catalogs, and stream audio directly — without any central server.

```
┌──────────────────┐         QUIC/UDP          ┌──────────────────┐
│  SoundTime A     │◄────────────────────────►  │  SoundTime B     │
│                  │                            │                  │
│  ┌────────────┐  │     ┌───────────────┐      │  ┌────────────┐  │
│  │ iroh Node  │──┼────►│  n0 Relay     │◄─────┼──│ iroh Node  │  │
│  └────────────┘  │     │  Servers      │      │  └────────────┘  │
│  ┌────────────┐  │     └───────────────┘      │  ┌────────────┐  │
│  │ Blob Store │  │                            │  │ Blob Store │  │
│  │  (BLAKE3)  │  │                            │  │  (BLAKE3)  │  │
│  └────────────┘  │                            │  └────────────┘  │
└──────────────────┘                            └──────────────────┘
```

### Key Technologies

| Component | Technology | Purpose |
|-----------|-----------|---------|
| Transport | iroh 1.5 (QUIC over UDP) | Encrypted peer-to-peer connections |
| Content storage | iroh-blobs 0.105 (BLAKE3, FsStore) | Content-addressed audio file storage |
| Peer discovery | Public listing, rendezvous, LAN announcements, seeds and peer exchange | Finding SoundTime NodeIds |
| Address lookup | DNS, Pkarr, optional Mainline DHT and mDNS | Connecting to known NodeIds |
| Relay | n0.computer production relays | NAT traversal when direct connections fail |
| Search | Bloom filters (~1.2 MB per peer) | Distributed query routing |
| Health | TrackHealthManager | Auto-retry, 3-strike dereference, re-referencing |

## Identity

Each SoundTime instance has a persistent **Ed25519 keypair** that defines its **NodeId** (a public key). The secret key is stored at the path defined by `P2P_SECRET_KEY_PATH` (default: `data/p2p/secret_key`).

- Generated automatically on first run using OS-level randomness
- Persists across restarts to maintain a stable identity
- The NodeId is used to address peers and is shared when connecting

> **Tip**: Back up the secret key file to preserve your node's identity when migrating servers.

## Protocol

SoundTime uses a custom application protocol identified by the ALPN `soundtime/p2p/1`. Messages are transmitted as **length-prefixed JSON** over QUIC bidirectional streams:

```
┌──────────────────────────────────────┐
│  4 bytes (big-endian u32)  │  JSON  │
│  ── message length ──────  │ payload│
└──────────────────────────────────────┘
```

### Message Types

| Message | Direction | Description |
|---------|-----------|-------------|
| `Ping` | → | Discovery probe, initiates handshake |
| `Pong` | ← | Response with sender's NodeId and track count |
| `AnnounceTrack` | → | Push a single track's metadata to a peer |
| `CatalogSync` | → | Batch push of all locally-uploaded tracks |
| `CatalogDelta` | → | Incremental sync — only new tracks since last sync |
| `FetchTrack` | → | Request a track blob by BLAKE3 hash |
| `TrackData` | ← | Response with track blob data |
| `PeerExchange` | ↔ | Share list of known peer NodeIds |
| `BloomFilterExchange` | ↔ | Exchange search Bloom filters for query routing |
| `SearchQuery` | → | Distributed search request (text query) |
| `SearchResults` | ← | Matching tracks from a peer's catalog |

### Track Announcement

When a track is announced (via `AnnounceTrack` or `CatalogSync`), the following metadata is included:

```json
{
  "hash": "blake3-content-hash",
  "title": "Song Title",
  "artist_name": "Artist",
  "album_title": "Album",
  "duration_secs": 240.5,
  "format": "flac",
  "file_size": 30000000,
  "genre": "Electronic",
  "year": 2024,
  "track_number": 1,
  "disc_number": 1,
  "bitrate": 1411,
  "sample_rate": 44100,
  "origin_node": "originating-node-id",
  "cover_hash": "blake3-cover-hash"
}
```

### Artist credits across versions

An upload or scan with no artist credit has no artist label in the current HTTP catalog or player. Artists explicitly named `Unknown Artist` retain their own profile and are searchable under that name. Existing literal names in an older library may come from either an artist tag or the former upload fallback. The upgrade preserves ambiguous names and their IDs.

V1 announcements send an empty `artist_name` for an absent credit. The archived v1 backend accepts that string. Signed v2 manifests have always required a nonempty `artist`, so the origin encodes an absent credit as one ASCII space in a new journal revision. Current receivers map that wire value to the internal placeholder and omit its label from search and track responses. Previously signed revisions retain their original bytes. An old issuer that sent the literal `Unknown Artist` supplied no way to distinguish those two meanings. The `compatibility` and `historical` bench profiles exchange an uncredited WAV and an explicitly credited homonym and inspect each receiver's public track response.

## Upgrading the P2P store

This release uses iroh 1.5 and iroh-blobs 0.105. The new backend can read tags, inlined blobs and external audio from the previous 0.96 store. Opening the index changes redb's allocator format. The old backend cannot reopen the upgraded index. SoundTime leaves an existing, unmarked store untouched and serves the application with P2P disabled until the operator confirms a backup. Fresh stores and subsequent restarts require no action.

With the standard Compose deployment, run the following **before replacing the old backend image**, from the directory containing `docker-compose.yml`. Keep the original image on this host and protect the archive, which contains the node's private key. If the private key is configured outside the P2P volume, include that key in your regular backup as well.

```sh
set -e
backup=$(mktemp -d "./soundtime-p2p-backup.XXXXXX")
old_image="soundtime-backend:before-p2p-$(date -u +%Y%m%d%H%M%S)"
docker tag "$(docker inspect --format '{{.Image}}' "$(docker compose ps -q backend)")" "$old_image"
echo "$old_image" > "$backup/image"
docker compose stop backend
docker compose run --rm --no-deps -T --user 0 --entrypoint tar backend \
  -C /data/p2p-blobs -cf - . > "$backup/p2p.tar"
tar -tf "$backup/p2p.tar" > "$backup/files"
grep -Fx './blobs.db' "$backup/files"
docker compose run --rm --no-deps -T --user 0 --entrypoint touch backend \
  /data/p2p-blobs/.soundtime-blobs-0.105
docker compose pull backend
docker compose up -d backend
```

Allow space for the entire store. The archive includes external blob files that garbage collection may subsequently remove. Keep a matching PostgreSQL backup for a whole-application rollback. For a native installation, stop the server, snapshot `P2P_BLOBS_DIR` and its key, then create the same empty marker in the snapshotted source directory before starting the new binary.

To roll back the P2P component to the backup point, restore the archive into a **new empty volume** so the upgraded data is still available. Set `backup` to the directory created above in the same shell. Restore the corresponding application/database version as appropriate.

```sh
set -e
docker compose stop backend
export SOUNDTIME_ROLLBACK_IMAGE="$(cat "$backup/image")"
export SOUNDTIME_ROLLBACK_VOLUME="$(docker volume create)"
docker run --rm -i --user 0 --entrypoint tar \
  -v "$SOUNDTIME_ROLLBACK_VOLUME:/restore" "$SOUNDTIME_ROLLBACK_IMAGE" \
  -C /restore -xf - < "$backup/p2p.tar"
cat > "$backup/rollback.yml" <<'YAML'
services:
  backend:
    image: ${SOUNDTIME_ROLLBACK_IMAGE}
    volumes:
      - restored_p2p:/data/p2p-blobs
volumes:
  restored_p2p:
    external: true
    name: ${SOUNDTIME_ROLLBACK_VOLUME}
YAML
docker compose -f docker-compose.yml -f "$backup/rollback.yml" up -d --no-deps backend
```

Use the same override and exported variables for subsequent commands on the rolled-back deployment. The restored volume reflects the snapshot time. Newer blobs remain on the upgraded volume. The SoundTime ALPN stays `soundtime/p2p/1`. Direct QUIC connections were tested with a 0.96 and a 1.5 endpoint in both directions. Operators of relay-dependent nodes should upgrade both participants. Public relay service for the old canary versions ended on September 30, 2026. [Iroh public relay support](https://www.iroh.computer/blog/v1#public-relay-support)

## Peer Discovery

SoundTime uses multiple discovery mechanisms to find peers:

### 1. n0 DNS Discovery (default)

The primary discovery method. Your node publishes its NodeId and relay URL to n0's DNS infrastructure using `PkarrPublisher`, and resolves other nodes via `DnsDiscovery`.

- **Automatic** — no configuration needed
- Works across NATs via relay servers
- NodeIds are resolvable globally

### 2. Resolve NodeIds on the Mainline DHT

Iroh publishes and resolves addresses for known NodeIds through Pkarr. This includes relay addresses. The Mainline DHT does not provide a directory of SoundTime nodes. Set `P2P_DHT_DISCOVERY=false` and restart the backend to turn off this address lookup.

### Discover other SoundTime instances through a rendezvous

Two instances can find each other even if both use an iroh relay and neither has a public domain. Their operators must choose the same rendezvous service. Any SoundTime backend can host it. The directory is separate from the public website listing.

1. On a SoundTime backend reachable by both participants, set `P2P_RENDEZVOUS_SERVER=true` and recreate the backend with `docker compose up -d backend`. Configure HTTPS on this host. The directory can run with its own P2P node disabled.
2. On **each** participating instance, set `P2P_ENABLED=true` and `P2P_RENDEZVOUS_URL=https://music.example.org/api/p2p/rendezvous`, replacing the example host with your directory host. Recreate each backend. The participants may keep `SOUNDTIME_DOMAIN=localhost` and leave `P2P_SEED_PEERS` empty.
3. In explicit catalogue mode, enable `P2P_RENDEZVOUS_ANNOUNCE=true` on the instances that want to be found. They sign their registrations. Other instances read the directory with HTTP GET, including operators who keep their instance private. Check **Admin → P2P → Peers** for authenticated peers and choose which catalogues to follow.

The directory sees each caller's IP address. Only registrations include the caller's NodeId. Each reply samples up to eight current entries. Registrations expire 15 minutes after signing and disappear on a directory restart. Keep host clocks synchronized. Legacy clients continue to register when configured with a rendezvous URL. Read-only discovery requires a server supporting GET.

### Read the public SoundTime listing

In explicit catalogue mode, the instance periodically reads `GET /api/nodes` on the configured SoundTime listing service. It samples up to eight online, P2P-enabled nodes with valid NodeIds and authenticates each candidate over iroh. It uses the normal signed-in administrator controls for catalogue subscriptions. Reading the listing transmits no registration or NodeId to the listing service. Publishing your own domain still requires enabling the separate public listing setting. The setting `P2P_LISTING_DISCOVERY` controls reading. The public lookup default follows `P2P_PUBLIC_DISCOVERY`. [SoundTime Listing API](https://github.com/CICCADA-CORP/SoundTime-Listing/blob/main/server.js)

### 3. Local Swarm Discovery (mDNS)

Optional LAN-based discovery for instances on the same network.

```env
P2P_LOCAL_DISCOVERY=true
```

Explicit catalogue instances listen for LAN announcements and authenticate new SoundTime peers. Enable `P2P_LOCAL_ADVERTISE=true` on at least one instance to make it visible on the LAN. Browsing, LAN advertising, signed rendezvous registration and consent for other peers to redistribute your NodeId are separate choices. Each peer you contact sees your authenticated identity. Legacy mode retains LAN advertising when local discovery is enabled.

### 4. Seed Peers

Explicitly configured peers that are connected on startup:

```env
P2P_SEED_PEERS=node_id_1,node_id_2,node_id_3
```

On startup, the node authenticates available seeds. In explicit catalogue mode it resumes existing subscriptions. Legacy mode also synchronizes the catalogue.

### 5. Peer Exchange (PEX)

Compatible peers exchange a bounded sample of consenting identities every five minutes. Enable `P2P_SHARE_IDENTITY=true` to allow this redistribution. In explicit catalogue mode each instance admits up to 32 new discovery probes per minute across PEX, rendezvous, the listing and LAN announcements. Repeated hints about an identity share one attempt per minute. A successful authenticated probe records presence, and a persistent ban prevents a new probe.

The public network graph shows a sample of neighbors that permitted redistribution. The administrator's graph includes the full local registry. A private instance requires login for P2P status and the graph. Its public login metadata exposes its NodeId only when the operator enables public listing.

```
Node A ──PEX──► Node B ──PEX──► Node C
   │                                │
   └────── discovers C via B ───────┘
```

## Catalog Synchronization

### Upload Flow

When a user uploads a track:

1. Audio file is saved to local storage
2. Metadata is extracted (lofty) and stored in PostgreSQL
3. The audio file is published to the iroh-blobs store (BLAKE3 hash computed)
4. If the album has cover art, the cover is also published to the blob store
5. An `AnnounceTrack` message is broadcast to **all connected peers**

### Receiving Announcements

When a peer receives a track announcement:

1. Check for duplicates by `content_hash` in the local database
2. If new: fetch the blob from the announcing peer via iroh-blobs
3. Create local database records (artist → album → track → remote_track)
4. The track's file path is stored as `p2p://<blake3-hash>`
5. If `cover_hash` is present, fetch and save the cover art locally

### Full Catalog Sync

When a new peer connects (via `Ping`/`Pong` handshake), the responding node automatically sends a `CatalogSync` message containing **all locally-uploaded tracks**. This ensures new peers quickly receive the full library.

### Incremental Sync

After the initial full sync, subsequent syncs use `CatalogDelta` messages that contain **only new tracks** since the last sync. This avoids redundant data transfer and scales well as libraries grow.

### Cover Art Sync

Cover art is synchronized alongside tracks:

- Cover images are published to the iroh-blobs store with their own BLAKE3 hash
- The `cover_hash` field in `TrackAnnouncement` links the cover to its track
- Receiving peers save covers to `<storage_path>/p2p-covers/<artist>/<album>/cover.jpg`
- Album records are updated with the local cover URL

## Content-Addressed Storage

SoundTime uses **iroh-blobs** for content-addressed storage:

- Every file (audio + covers) is identified by its **BLAKE3 hash**
- Duplicate content is automatically deduplicated
- Files can be verified for integrity at any time
- Blob data is persisted to disk at `P2P_BLOBS_DIR`

```
data/p2p/
├── blobs/          # iroh-blobs persistent store (BLAKE3-indexed)
└── secret_key      # Ed25519 node identity
```

## NAT Traversal & Relays

SoundTime handles NAT traversal automatically:

1. **Direct connection** — Attempted first via QUIC hole-punching
2. **Relay fallback** — If direct connection fails, traffic is routed through [n0.computer](https://n0.computer/) production relay servers

The relay connection is established on startup (with a 15-second timeout). If no relay is available, the node falls back to direct connections only.

> **Note**: For best performance, open UDP port **11204** in your firewall to allow direct connections.

## Track Health Monitoring

SoundTime automatically monitors the health of remote P2P tracks and repairs them when possible.

### Health States

| Status | Description |
|--------|-------------|
| **Healthy** | Track blob is available locally, no issues |
| **Recovered** | Track was unavailable but successfully re-fetched |
| **Degraded** | 1–2 failed attempts, still attempting recovery |
| **Dereferenced** | 3+ consecutive failures, track marked unavailable |

### Auto-Repair Flow

When a track fails to play:

1. **Origin retry** — Re-fetch from the original peer that announced the track
2. **Alternative peers** — If origin fails, try other peers that have the same content hash
3. **Strike counter** — Increment failure count; after 3 strikes → dereference
4. **Automatic re-referencing** — If a dereferenced track's blob becomes locally available (e.g., peer comes back online), the track is automatically restored to Healthy status

### Background Health Sweep

A background task runs periodically (default: every 10 minutes) to proactively check remote tracks:

- Processes tracks in batches (default: 500 per sweep)
- Checks local blob availability via iroh-blobs
- Attempts recovery for degraded tracks
- Re-references dereferenced tracks when their blob reappears
- Persists health state changes to the database

### Duplicate Resolution

When the same track exists on multiple peers, `select_best_copy` ranks copies by:

1. **Peer online status** — Online peers preferred over offline
2. **Audio format** — FLAC > WAV > OPUS > OGG > AAC > MP3
3. **Bitrate** — Higher bitrate scores better (capped at 1411 kbps)
4. **Sample rate** — Higher sample rates get a bonus

## Distributed Search

SoundTime uses Bloom filter-based routing for efficient distributed search.

### How It Works

1. Each node builds a **Bloom filter** (~1.2 MB) from its local track metadata (titles, artists, albums)
2. Bloom filters are exchanged between peers via `BloomFilterExchange` messages
3. When a user searches, the query terms are checked against each peer's Bloom filter
4. Only peers whose filter matches receive the `SearchQuery` message
5. Matching peers respond with `SearchResults` containing matching tracks

This avoids flooding the network with search requests — only relevant peers are queried.

### Parameters

- **Filter size**: 100,000 entries capacity
- **False positive rate**: ~1%
- **Serialized size**: ~1.2 MB per peer
- **Term normalization**: Lowercase, word splitting, short words (< 2 chars) filtered out

## Peer Blocking

Peers can be blocked from the admin panel or API. Blocked peers:

- Are rejected on incoming connections
- Cannot fetch your tracks
- Are excluded from PEX announcements

Blocking uses the `blocked_domains` database table, where the `domain` column stores the iroh NodeId.

```bash
# Block a peer via API
curl -X POST http://localhost:8080/api/admin/blocked-domains \
  -H "Authorization: Bearer <admin_token>" \
  -H "Content-Type: application/json" \
  -d '{"domain": "peer-node-id-to-block"}'
```

## Configuration Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `P2P_ENABLED` | `true` | Enable/disable the P2P node |
| `P2P_PORT` | `11204` | QUIC listening port (UDP) |
| `P2P_BIND_PORT` | `11204` | Bind port (0 = random) |
| `P2P_BLOBS_DIR` | `data/p2p/blobs` | iroh-blobs persistent storage path |
| `P2P_SECRET_KEY_PATH` | `data/p2p/secret_key` | Path to the Ed25519 secret key |
| `P2P_PUBLIC_DISCOVERY` | `true` | Use the public iroh lookups and relays. Turning it off also disables Mainline DHT. LAN discovery and explicitly configured rendezvous are independent. |
| `P2P_RELAY_URL` | empty | Use one explicitly configured HTTP(S) iroh relay and seed known peers on it. |
| `P2P_RELAY_ONLY` | `false` | Disable direct IP transports and use only the configured relay. |
| `P2P_EXPLICIT_CATALOG` | `false` | Opt in to the control protocol which separates presence and catalogue subscription. |
| `P2P_VERSIONED_CATALOG` | `false` | Enable signed publication inventories on peers that also enable explicit catalogues. |
| `P2P_SHARE_CACHED` | `false` | With versioned catalogues and range streaming enabled, announce renewable leases for complete verified cache admissions. |
| `P2P_SHARE_IDENTITY` | `false` | Give compatible peers permission to include this node's identity in their peer-exchange sample. |
| `P2P_DHT_DISCOVERY` | `true` | Resolve known NodeIds via Pkarr on the Mainline DHT |
| `P2P_RENDEZVOUS_URL` | — | Shared endpoint for opt-in discovery of SoundTime NodeIds |
| `P2P_RENDEZVOUS_ANNOUNCE` | `false` in explicit mode | Register your signed identity on the configured rendezvous. Legacy mode registers by default. |
| `P2P_RENDEZVOUS_SERVER` | `false` | Host the signed, expiring rendezvous directory |
| `P2P_LISTING_DISCOVERY` | Follows `P2P_PUBLIC_DISCOVERY` | Discover candidates from the configured public listing in explicit mode. |
| `P2P_LOCAL_DISCOVERY` | `true` | Listen for LAN peers via mDNS |
| `P2P_LOCAL_ADVERTISE` | `false` in explicit mode | Publish this instance's presence on the LAN. Legacy mode advertises by default when LAN discovery is enabled. |
| `P2P_SEED_PEERS` | — | Comma-separated NodeIds for auto-connect |

Run `scripts/test-p2p-federation.sh discovery` to exercise LAN discovery, a read-only rendezvous participant, explicit registration and an isolated copy of the public listing API. The three-instance scenario checks authenticated presence, zero catalogue imports and persistent peer blocking across a restart.

### Verified range streaming pilot

Set `P2P_RANGE_STREAMING=true` on participants to advertise `soundtime/range/2` alongside the existing control protocol. The default is `false`. The receiver needs a current catalogue entry with the origin, publication UUID, revision, BLAKE3 hash and size, and an unexpired provider offer. When the option is enabled, an identified publication requires this range protocol. A provider error returns HTTP 503 before the first verified window, or terminates an already started response if all providers fail later. Historical records without an identified publication use the legacy path.

The receiver validates Bao proofs for aligned 64 KiB windows before emitting any bytes. Concurrent requests for an overlapping window of the same publication share its in-flight QUIC stream. A slow or unavailable provider can be replaced at the current window. The pilot admits 32 active downloads and 32 uploads per process, with four per peer in each direction. Waiting windows are bounded at 512. Cancelling the last reader cancels the upstream operation. A partial range read creates no durable cache or offer.

Set `P2P_CACHE_MAX_SIZE` to the storage budget (for example `512MB` or `0`) and `P2P_CACHE_MIN_FREE` to the required free space on the P2P volume (default `256MB`). The cache reserves the file, its Bao outboard and allocation overhead before writing. It protects pinned files and current readers, removes unused files in least-recently-used order and rechecks space before admission. On restart it reconciles complete assets and drops interrupted staging directories. Old mixed iroh stores retain their original and cover tags. Database pages and unclassified legacy bytes count against the new budget. The administrator can see these amounts under Storage and set a persistent budget up to the configured ceiling. Existing pins or active reservations can prevent a reduction until they are released.

With `P2P_CACHE_ON_PLAY=true`, a complete sequential remote HTTP read can retain and verify a file when space permits. Enable `P2P_SHARE_CACHED=true` to advertise retained publications to authorized peers. Both switches default to false. A zero disk budget still serves verified windows in memory. Administrators can keep a track or an album from its page. The admin API supports GET, PUT and DELETE on `/api/admin/cache/tracks/{id}/pin` and `/api/admin/cache/albums/{id}/pin`. It reserves the album budget before fetching audio, reports HTTP 507 on insufficient disk and preserves overlapping track and album pins. Owner withdrawals apply to every retained version of that publication.

The authenticated transport statistics expose `range_windows_verified` and `range_proof_bytes_received`. The latter includes excess Bao leaf bytes outside a short requested window and framing in the Bao response. QUIC packet headers and retransmissions are excluded.

## Monitoring

### P2P Status API

```bash
curl http://localhost:8080/api/p2p/status
```

Returns:
```json
{
  "enabled": true,
  "node_id": "abcdef1234...",
  "relay_url": "https://relay.example.com",
  "relay_connected": true,
  "direct_addresses": 2,
  "peer_count": 3,
  "online_peer_count": 2,
  "dht_discovery_enabled": true,
  "rendezvous_configured": false
}
```

### Network Graph

The admin panel includes an interactive **D3.js force-directed graph** showing your P2P network topology. Access it from the admin dashboard or via:

```bash
curl http://localhost:8080/api/p2p/network-graph
```

### Peer Management

The optional catalogue pilot negotiates `soundtime/control/2` through QUIC. Two pilot nodes can exchange an authenticated presence message without sending track announcements. To discover without following, call `POST /api/admin/p2p/peers` with the node ID and `"follow_catalog": false`. To start or stop following, send `{"followed": true}` or `{"followed": false}` with `PUT /api/admin/p2p/peers/{node_id}/subscription`. The local subscription survives a restart. The admin transport status includes `explicit_catalog` and the process-wide `catalog_entries_received` counter.

The historic mode still sends its original ping and catalogues. A pilot connecting to an older host negotiates the original ALPN and requires an explicit local follow before sending its catalogue-producing ping. A pilot answering a legacy ping sends its catalogue only when it follows the caller. A separate legacy catalogue request remains supported. Compatible peers sample up to eight identities. Third-party entries need the owner's consent from authenticated presence and are filtered against the local bans. Older software can retain and forward identifiers it already learned.

Two versioned peers use `soundtime/catalog/2` after subscription. A snapshot has a fixed commit cursor and an explicit final page. The receiver uses its saved page number and publication UUID to resume, and removes missing offers only after accepting that final page. Each page holds up to 64 signed manifests and carries metadata only. A subsequent pull reads changes after the last committed cursor. The origin signs its publication identity and revision with its iroh key. A signed withdrawal can therefore arrive through another authorized provider. Retained playlists, libraries and favourites are checked periodically against reachable issuers and known providers. A disconnected instance acts on its last verified information until a path becomes available.

While an upgraded peer prepares its historical journal, adding it with catalogue follow or enabling its subscription returns HTTP 202. The peer stays online and the last complete inventory remains available under its existing leases. Synchronization resumes from the saved cursor automatically. Each instance checks up to eight online followed sources every 15 seconds and continues receiving remote updates while preparing its own journal.

Cache admission binds a complete verified file to the particular publication that was read or pinned. A local listener can keep using that admission after losing the supplier. With sharing enabled, the mirror signs its own one-hour lease for the publication, revision, hash and size. An origin can relay this lease to its other subscribers. A higher provider generation revokes an older lease on eviction; expiry limits the lifetime of an offer whose revocation was lost. Private instances restrict versioned catalogue and range export to peers their administrator follows. Administrators can inspect eligible signed providers with `GET /api/admin/p2p/tracks/{local_track_id}/mirrors`.

The admin endpoint `GET /api/admin/p2p/transport` reports the local transport's numeric addresses and cumulative audio payload bytes written to and read from QUIC since process startup. A partially transferred file contributes its accepted bytes. Covers, protocol headers and retransmissions are excluded. On a private network, an admin can pass a `direct_addresses` list (for example `["10.0.0.2:11204"]`) alongside the peer's `node_id`. The endpoint verifies the remote identity when establishing the connection. The explicit address list is kept in memory.

From the admin panel or API:

```bash
# List peers
curl http://localhost:8080/api/admin/p2p/peers \
  -H "Authorization: Bearer <token>"

# Add a peer
curl -X POST http://localhost:8080/api/admin/p2p/peers \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"node_id": "peer-node-id"}'

# Ping a peer
curl -X POST http://localhost:8080/api/admin/p2p/peers/<node_id>/ping \
  -H "Authorization: Bearer <token>"
```

## Troubleshooting

### Peers not connecting

1. **Check P2P is enabled**: Ensure `P2P_ENABLED=true`
2. **Check firewall**: Open UDP port 11204
3. **Check logs**: Set `RUST_LOG=info,soundtime_p2p=debug` for detailed P2P logging
4. **Relay status**: The node logs its relay URL on startup — verify it's connected
5. **Seed peers**: If using seed peers, verify the NodeIds are correct and the remote peers are online

### Tracks not syncing

1. **Check peer connection**: Ping the peer from admin panel
2. **Check blob store**: Verify `P2P_BLOBS_DIR` is writable and has disk space
3. **Check announcements**: Enable debug logging to see `AnnounceTrack` messages
4. **Duplicate check**: Tracks with the same `content_hash` are skipped (by design)

### High relay traffic

If most traffic goes through relays, direct connections may be failing:

1. Open UDP port 11204 on both ends
2. Check if your ISP supports QUIC (some block UDP)
3. Consider hosting peers on the same network for best performance

### Read replicas and a single P2P owner

Use the standalone configuration for one server. A deployment serving more catalogue reads can run the same backend image with the optional `docker/compose.public.yml` overlay. Both images must contain the federation migrations. For example, `docker compose -f docker-compose.yml -f docker/compose.public.yml up -d --scale catalog-http=2` starts two read processes. Supply the same database, signing secret and catalogue policy flags to each process.

Nginx distributes indexed GET requests for search, tracks, albums and artists across the replicas. Searches requesting external peers, account changes, uploads, media and cache operations go to the owner. Each catalogue replica defaults to eight PostgreSQL connections. Readers query the same primary and enforce the private-instance authentication middleware.

The public overlay gives the active worker 24 serving connections and one dedicated connection for migration and lease renewal. Two catalogue readers therefore use at most 41 application connections. PostgreSQL allows 64 connections, leaving 20 ordinary slots after its three default reserved slots. Each additional standby needs one lease connection. Each additional catalogue reader needs its own pool allowance. When scaling, keep the sum of the serving, reader and lease budgets below the available PostgreSQL slots including room for monitoring and maintenance. The overlay accepts `PUBLIC_WORKER_DB_MAX_CONNECTIONS`, `CATALOG_DB_MAX_CONNECTIONS` and `PUBLIC_POSTGRES_MAX_CONNECTIONS`.

PostgreSQL has a 256 MiB shared-memory mount for parallel query work, configurable through `PUBLIC_POSTGRES_SHM_SIZE`, and allows one parallel worker per gather. The mount is a memory ceiling and consumes space as PostgreSQL writes to it. Nginx allows 4,096 connections and 8,192 open file descriptors per worker. Each proxied audio response occupies a client and an upstream connection. These limits apply to the optional public profile.

An owner configured with `SOUNDTIME_ROLE=worker` acquires a database lease before opening its P2P identity and cache. A standby needs the same original files, blob volume, persistent secret key and service endpoint. In Docker, both owners can advertise the `backend` network alias to the SoundTime proxy. Nginx refreshes that alias using Docker DNS. The standby starts serving HTTP after it acquires the lease. An interrupted client can then resume by requesting the remaining byte range at its original URL.

On takeover PostgreSQL issues a higher generation and rejects writes from the former owner. PostgreSQL grants a 15-second lease. The owner retries transient renewal failures within 13 seconds of sending its last confirmed request, including the time spent opening its serving pool. A renewal that matches no row stops the owner immediately. Its supervisor can then restart it. Returning to the standalone role on a database with a lease also acquires a new generation. Roll back the feature flags using a binary that understands withdrawals and bans.

`P2P_MAX_RANGE_TRANSFERS` limits active verified transfers per direction and `P2P_MAX_RANGES_PER_PEER` limits one remote identity. Set `P2P_UPLOAD_BYTES_PER_SECOND` and `P2P_PEER_UPLOAD_BYTES_PER_SECOND` to bound encoded range responses. The corresponding download settings pace useful incoming audio. Each limited direction permits a burst of one encoded window. A rate of zero disables that rate limit. These settings cover the versioned range protocol. Legacy full-file transfers and browser-facing HTTP consume separate bandwidth.

### Original files and object storage

An HTTP read of an S3 original captures its version ID or ETag and sends one conditional range request for the selected bytes. A local or mounted original keeps an open file descriptor and SoundTime checks the file's size and timestamps after reading each window. A detected change ends that response. The next request opens a fresh version.

Scanning an S3 object for tags and waveform uses a temporary file under `S3_CACHE_PATH`. The defaults allow 512 MiB per analysis and keep 256 MiB free. Configure `S3_ANALYSIS_MAX_BYTES` and `S3_ANALYSIS_MIN_FREE_BYTES` in bytes. One scanner can hold the staging directory at a time. Completion and the next scan after an interruption reclaim the temporary files.

PostgreSQL stores the Bao verification tree for local publications (about 4 KiB per MiB of audio). Each 64 KiB peer request selects only the 64-byte hash pairs along its verification paths and reads the corresponding audio leaf groups from the filesystem or a conditional S3 range. A later replacement cannot supply bytes for the earlier revision. A persistent queue indexes historical originals once, including rows that already have a BLAKE3 hash. A move or a restored mount can resume indexing. A mismatch with an existing SHA-256 or BLAKE3 fingerprint makes the source unavailable and preserves the publication and its listeners' references.

Set `P2P_REFERENCE_IMMUTABLE_UPLOADS=true` only on an instance whose managed filesystem uploads remain immutable at their recorded paths. Large eligible uploads can then also be referenced in place by iroh-blobs. The legacy blob volume counts existing payloads and proofs toward its physical usage, including copies protected by a publication tag.
