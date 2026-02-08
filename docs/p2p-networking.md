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
| Transport | iroh 0.32 (QUIC over UDP) | Encrypted peer-to-peer connections |
| Content storage | iroh-blobs (BLAKE3) | Content-addressed audio file storage |
| Discovery | n0 DNS + optional mDNS | Finding peers on the network |
| Relay | n0.computer production relays | NAT traversal when direct connections fail |

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
| `FetchTrack` | → | Request a track blob by BLAKE3 hash |
| `TrackData` | ← | Response with track blob data |
| `PeerExchange` | ↔ | Share list of known peer NodeIds |

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

## Peer Discovery

SoundTime uses multiple discovery mechanisms to find peers:

### 1. n0 DNS Discovery (default)

The primary discovery method. Your node publishes its NodeId and relay URL to n0's DNS infrastructure using `PkarrPublisher`, and resolves other nodes via `DnsDiscovery`.

- **Automatic** — no configuration needed
- Works across NATs via relay servers
- NodeIds are resolvable globally

### 2. Local Swarm Discovery (mDNS)

Optional LAN-based discovery for instances on the same network.

```env
P2P_LOCAL_DISCOVERY=true
```

Uses `iroh::discovery::local_swarm_discovery::LocalSwarmDiscovery` to find peers via multicast DNS. Useful for home lab setups where multiple instances run on the same network.

### 3. Seed Peers

Explicitly configured peers that are connected on startup:

```env
P2P_SEED_PEERS=node_id_1,node_id_2,node_id_3
```

On startup, the node:
1. Pings each seed peer
2. Registers them in the peer registry
3. Exchanges peer lists (PEX)
4. Sends a full catalog sync

### 4. Peer Exchange (PEX)

Peers periodically share their known peer lists with each other:

- Runs every **5 minutes** on a background timer
- Sends `PeerExchange` messages containing all known peer NodeIds
- Newly discovered peers are automatically pinged and registered
- Creates a gossip-like mesh for organic peer discovery

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
| `P2P_LOCAL_DISCOVERY` | `true` | Enable mDNS local network discovery |
| `P2P_SEED_PEERS` | — | Comma-separated NodeIds for auto-connect |

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
  "peers_count": 3
}
```

### Network Graph

The admin panel includes an interactive **D3.js force-directed graph** showing your P2P network topology. Access it from the admin dashboard or via:

```bash
curl http://localhost:8080/api/p2p/network-graph
```

### Peer Management

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
