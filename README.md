<p align="center">
  <img src="docs/assets/logo.png" alt="SoundTime Logo" width="120" />
</p>

<h1 align="center">SoundTime</h1>

<p align="center">
  <strong>Self-hosted music streaming with peer-to-peer sharing</strong>
</p>

<p align="center">
  <a href="https://github.com/CICCADA-CORP/SoundTime/actions/workflows/ci.yml"><img src="https://github.com/CICCADA-CORP/SoundTime/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/version-1.0.0-green.svg" alt="Version" />
  <img src="https://img.shields.io/badge/rust-1.78%2B-orange.svg" alt="Rust" />
  <img src="https://img.shields.io/badge/node-20%2B-339933.svg" alt="Node" />
</p>

<p align="center">
  <a href="#features">Features</a> •
  <a href="#screenshots">Screenshots</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#architecture">Architecture</a> •
  <a href="docs/api-reference.md">API Reference</a> •
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

---

SoundTime is a self-hosted music streaming platform built with **Rust** and **SvelteKit**. Upload your music library, organize it into playlists, and share tracks across instances using **peer-to-peer networking** powered by [iroh](https://iroh.computer/) (by [n0.computer](https://n0.computer/)).

Unlike centralized platforms, SoundTime gives you full control over your music. Run it on your own server, connect with other SoundTime instances through encrypted P2P channels, and enjoy a modern, responsive listening experience.

## Features

### 🎵 Music Streaming
- **Upload & organize** — Drag-and-drop upload with automatic metadata extraction (artist, album, track number, cover art)
- **Adaptive streaming** — OPUS transcoding at 320/128/64 kbps with server-side caching
- **Waveform visualization** — Real-time audio waveform display powered by Symphonia
- **Lyrics support** — Fetch and display lyrics from multiple providers (Musixmatch, Lyrics.com)
- **Full-text search** — Search across tracks, albums, and artists instantly

### 📚 Library Management
- **Albums & artists** — Auto-organized from metadata with cover art support
- **Playlists** — Create, edit, and share public or private playlists
- **Personal libraries** — Curate your own collection from available tracks
- **Favorites & history** — Track your listening habits and bookmark songs
- **Batch upload** — Upload entire albums or folders at once

### 🌐 Peer-to-Peer Network
- **iroh-powered P2P** — Encrypted QUIC connections via [iroh](https://iroh.computer/) for peer discovery and track sharing
- **Relay support** — NAT traversal through [n0.computer](https://n0.computer/) production relay servers
- **Content-addressed storage** — Tracks identified by BLAKE3 hashes via iroh-blobs
- **Network visualization** — Interactive D3.js force-directed graph of your P2P network topology
- **Peer management** — Add, ping, and manage connected peers from the admin panel

### 🛡️ Security & Privacy
- **Argon2id** password hashing (OWASP-recommended)
- **JWT authentication** — Short-lived access tokens (15 min) + refresh tokens (7 days)
- **Rate limiting** — Per-IP throttling on auth endpoints via tower-governor
- **Security headers** — HSTS, X-Content-Type-Options, X-Frame-Options
- **CORS controls** — Configurable allowed origins

### 🤖 AI-Powered Features
- **Editorial playlists** — AI-generated curated playlists based on your library (OpenAI-compatible APIs)
- **Smart metadata enrichment** — Automatic metadata lookup via MusicBrainz

### 🌍 Internationalization
- 5 languages out of the box: **English**, **Français**, **Español**, **中文**, **Русский**
- Auto-detection from browser language
- Easy to add new translations

### 🔧 Administration
- **Dashboard** — Track counts, user stats, storage status at a glance
- **User management** — Roles (admin/user), banning, moderation
- **Content moderation** — Report system with review workflow
- **Storage management** — Integrity checks, S3 sync, filesystem monitoring
- **Instance settings** — Configurable from the admin panel
- **Terms of Service** — Customizable ToS with editor

## Screenshots

<!-- TODO: Replace with actual screenshots of the SoundTime UI -->
<!-- Recommended: dashboard, player with waveform, admin panel, network graph -->

<p align="center">
  <em>Screenshots coming soon — see <a href="#quick-start">Quick Start</a> to try it yourself!</em>
</p>

<!--
<p align="center">
  <img src="docs/assets/screenshot-player.png" alt="Audio Player" width="800" />
</p>
<p align="center">
  <img src="docs/assets/screenshot-admin.png" alt="Admin Dashboard" width="800" />
</p>
<p align="center">
  <img src="docs/assets/screenshot-network.png" alt="P2P Network Graph" width="800" />
</p>
-->

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Backend** | Rust, [Axum](https://github.com/tokio-rs/axum) 0.8, [Sea-ORM](https://www.sea-ql.org/SeaORM/) 1.1, PostgreSQL 16 |
| **Frontend** | [SvelteKit](https://kit.svelte.dev/) 2, Svelte 5, Tailwind CSS, shadcn-svelte |
| **Auth** | Argon2id, JWT (jsonwebtoken), tower-governor |
| **Audio** | [Lofty](https://github.com/Serial-ATA/lofty-rs) (metadata), [Symphonia](https://github.com/pdeljanov/Symphonia) (decode/waveform) |
| **P2P** | [iroh](https://iroh.computer/) 0.32 (QUIC), iroh-blobs (content-addressed storage) |
| **Visualization** | [D3.js](https://d3js.org/) 7 (network graph) |
| **Storage** | Local filesystem or AWS S3-compatible |
| **Deployment** | Docker Compose, Nginx reverse proxy |

## Quick Start

### Docker Compose (recommended)

The fastest way to get SoundTime running:

```bash
git clone https://github.com/CICCADA-CORP/SoundTime.git
cd SoundTime

# Configure environment
cp .env.example .env
# Edit .env — at minimum, change JWT_SECRET to a random string

# Launch all services
docker compose up --build
```

Once started:
- 🎵 **Frontend**: http://localhost:3000
- 🔌 **API**: http://localhost:8080
- 🌐 **Nginx proxy**: http://localhost:8880

The first user to register automatically becomes the **admin**. Open the frontend and create your account to begin the setup wizard.

### Local Development

See the [Development Guide](docs/development.md) for a complete setup walkthrough.

**Prerequisites**: Rust 1.78+, Node.js 20+, PostgreSQL 16

```bash
# Start PostgreSQL
docker compose up postgres -d

# Configure environment
cp .env.example .env

# Backend (terminal 1)
cd backend
cargo run

# Frontend (terminal 2)
cd frontend
npm install
npm run dev
```

### Production Deployment

See the [Deployment Guide](docs/deployment.md) for production setup with SSL, custom domain, S3 storage, and P2P configuration.

## Architecture

SoundTime follows a modular monorepo architecture with a Rust backend organized into 5 specialized crates:

```
soundtime/
├── backend/
│   └── crates/
│       ├── soundtime-server     # Axum HTTP server, routes, auth, middleware
│       ├── soundtime-db         # Sea-ORM entities & database connection pool
│       ├── soundtime-migration  # 22 database migrations (PostgreSQL)
│       ├── soundtime-audio      # Audio metadata, storage, waveform generation
│       └── soundtime-p2p        # P2P networking via iroh (discovery, blobs)
├── frontend/
│   └── src/
│       ├── lib/
│       │   ├── components/      # 11 UI components (AudioPlayer, NetworkGraph…)
│       │   ├── stores/          # Svelte 5 rune stores (auth, player, queue)
│       │   └── i18n/            # 5 language packs
│       └── routes/              # 16 SvelteKit pages
├── docker/                      # Dockerfiles + Nginx config
├── docs/                        # Documentation
└── docker-compose.yml           # Full-stack orchestration
```

```
┌─────────────┐     ┌──────────────────────────────────────────────┐
│   Browser    │────▶│              Nginx (reverse proxy)           │
└─────────────┘     └────────┬──────────────────────┬──────────────┘
                             │                      │
                    ┌────────▼────────┐    ┌────────▼────────┐
                    │    Frontend     │    │     Backend      │
                    │   SvelteKit 2   │    │    Axum 0.8      │
                    │   Port 3000     │    │    Port 8080     │
                    └─────────────────┘    └────────┬─────────┘
                                                    │
                                    ┌───────────────┼───────────────┐
                                    │               │               │
                           ┌────────▼──┐   ┌───────▼──┐   ┌───────▼──────┐
                           │ PostgreSQL │   │  Audio   │   │   P2P Node   │
                           │   16      │   │ Storage  │   │  iroh/QUIC   │
                           └───────────┘   └──────────┘   └──────┬───────┘
                                                                 │
                                                        ┌────────▼────────┐
                                                        │ n0.computer     │
                                                        │ Relay Servers   │
                                                        └─────────────────┘
```

For a deep dive, see the [Architecture Guide](docs/architecture.md).

## Documentation

| Guide | Description |
|-------|-------------|
| [Architecture](docs/architecture.md) | System design, crate responsibilities, data flow |
| [API Reference](docs/api-reference.md) | Complete REST API documentation (60+ endpoints) |
| [Deployment](docs/deployment.md) | Production setup, SSL, S3, environment variables |
| [Development](docs/development.md) | Local dev setup, testing, project structure |
| [P2P Networking](docs/p2p-networking.md) | iroh protocol, relay servers, content addressing |

## Contributing

We welcome contributions of all kinds! Whether you're fixing a typo, adding a feature, or improving documentation — every contribution matters.

- 📖 Read the [Contributing Guide](CONTRIBUTING.md)
- 🐛 Found a bug? [Open an issue](https://github.com/CICCADA-CORP/SoundTime/issues/new?template=bug_report.md)
- 💡 Have an idea? [Request a feature](https://github.com/CICCADA-CORP/SoundTime/issues/new?template=feature_request.md)
- 🔐 Security issue? See our [Security Policy](SECURITY.md)

## Community

- 📋 [GitHub Issues](https://github.com/CICCADA-CORP/SoundTime/issues) — Bug reports & feature requests
- 💬 [GitHub Discussions](https://github.com/CICCADA-CORP/SoundTime/discussions) — Questions & ideas

## Security

If you discover a security vulnerability, please report it responsibly. See [SECURITY.md](SECURITY.md) for details.

## License

SoundTime is licensed under the [GNU Affero General Public License v3.0](LICENSE).

This means you can use, modify, and distribute SoundTime freely, but if you run a modified version as a network service, you must make the source code available to its users.

---

<p align="center">
  Made with ❤️ by <a href="https://github.com/CICCADA-CORP">CICCADA</a>
</p>
