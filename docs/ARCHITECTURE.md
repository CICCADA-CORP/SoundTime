# Architecture Overview

SoundTime follows a modern, distributed architecture designed for self-sovereignty and performance.

## System Components

```mermaid
graph TD
    User[User Browser]
    subgraph "SoundTime Node"
        Proxy[Nginx Proxy]
        Frontend[SvelteKit App]
        Backend[Rust API Server]
        DB[(PostgreSQL)]
        P2P[Iroh P2P Service]
        FS[File System]
    end
    
    User --> Proxy
    Proxy --> Frontend
    Proxy --> Backend
    
    Frontend --> Backend
    Backend --> DB
    Backend --> FS
    Backend --> P2P
    
    P2P <--> Internet[Other Nodes]
```

## Backend (`crates/soundtime-server`)

The core server is written in **Rust** for safety and speed.

- **Framework**: `axum` (0.7+)
- **Database ORM**: `sea-orm` (async, dynamic)
- **Audio Processing**: 
  - `symphonia` for decoding/transcoding
  - `lofty` for metadata parsing
- **Auth**: Argon2id + JWT (Access/Refresh token rotation)

## Frontend (`frontend`)

The user interface is a Single Page Application (SPA) powered by SvelteKit.

- **Framework**: `SvelteKit 2` + `Svelte 5` (Runes)
- **Styling**: `TailwindCSS`
- **Components**: `shadcn-svelte`
- **State Management**: Svelte 5 Runes for global stores (Player, Queue)

## Peer-to-Peer (`crates/soundtime-p2p`)

We use **Iroh** for secure, direct device-to-device communication. This replaces our legacy ActivityPub implementation to offer better performance for large media files.

- **Discovery**: Local network (mDNS) + DHT
- **Transfer**: BLAKE3 verified streaming
- **Identity**: Ed25519 keypairs per node

## Database Schema

We use PostgreSQL 16. Key tables include:

- `user`: Account credentials and profile.
- `track`: Audio file metadata (title, artist, duration).
- `album`: Collection grouping.
- `playlist`: User-curated lists.
- `device`: Registered P2P devices for sync.

For full schema details, see `migration/src/m20220101_000001_create_table.rs`.
