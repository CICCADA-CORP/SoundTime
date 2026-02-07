# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2024-05-20

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
