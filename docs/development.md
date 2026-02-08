# Development Guide

Set up SoundTime for local development.

## Prerequisites

| Tool | Version | Installation |
|------|---------|-------------|
| **Rust** | 1.78+ | [rustup.rs](https://rustup.rs) |
| **Node.js** | 20+ | [nvm](https://github.com/nvm-sh/nvm) or [nodejs.org](https://nodejs.org) |
| **PostgreSQL** | 16 | Via Docker (recommended) or native install |
| **Docker** | Latest | [docker.com](https://docs.docker.com/get-docker/) |

## Quick Setup

### 1. Clone the repository

```bash
git clone https://github.com/CICCADA-CORP/SoundTime.git
cd SoundTime
```

### 2. Start PostgreSQL

The easiest way is using Docker:

```bash
docker compose up postgres -d
```

This starts a PostgreSQL 16 instance with the credentials from `.env.example`.

### 3. Configure environment

```bash
cp .env.example .env
```

The default `.env.example` values work for local development. Key variables:

```env
DATABASE_URL=postgres://soundtime:soundtime@localhost:5432/soundtime
JWT_SECRET=dev-secret-change-in-production
AUDIO_STORAGE_PATH=./data/music
P2P_ENABLED=true
RUST_LOG=info,soundtime=debug
```

> **Note**: The `JWT_SECRET` value `dev-secret-change-in-production` is accepted in development but will cause a panic if `SOUNDTIME_ENV=production`.

### 4. Run the backend

```bash
cd backend
cargo run
```

On first run, the server will:
1. Connect to PostgreSQL
2. Run all database migrations automatically
3. Initialize the storage backend
4. Start the P2P node (if `P2P_ENABLED=true`)
5. Listen on `http://0.0.0.0:8080`

### 5. Run the frontend

In a separate terminal:

```bash
cd frontend
npm install
npm run dev
```

The frontend starts at `http://localhost:5173` (Vite dev server) and proxies API requests to the backend at `http://localhost:8080`.

### 6. Create your account

Open `http://localhost:5173` in your browser. The first user to register becomes the **admin**.

## Project Structure

```
soundtime/
├── backend/
│   └── crates/
│       ├── soundtime-server/      # Axum HTTP server
│       │   └── src/
│       │       ├── main.rs        # Entry point, router, middleware
│       │       ├── api/           # Route handlers (tracks, auth, admin, etc.)
│       │       ├── auth/          # JWT, password hashing, middleware
│       │       └── errors.rs      # Error types
│       ├── soundtime-db/          # Sea-ORM entities & database
│       │   └── src/
│       │       ├── entities/      # Auto-generated Sea-ORM entities
│       │       └── lib.rs         # AppState, connection pool
│       ├── soundtime-migration/   # Database migrations
│       │   └── src/               # 22 migration files
│       ├── soundtime-audio/       # Audio processing
│       │   └── src/
│       │       ├── metadata.rs    # Lofty metadata extraction
│       │       ├── storage.rs     # File storage backends
│       │       └── waveform.rs    # Symphonia waveform generation
│       └── soundtime-p2p/         # P2P networking
│           └── src/
│               ├── node.rs        # iroh node, message handling
│               └── lib.rs         # Config, types
├── frontend/
│   └── src/
│       ├── lib/
│       │   ├── components/        # UI components
│       │   │   ├── AudioPlayer.svelte
│       │   │   ├── NetworkGraph.svelte
│       │   │   └── ...
│       │   ├── stores/            # Svelte 5 rune stores
│       │   │   ├── auth.svelte.ts
│       │   │   ├── player.svelte.ts
│       │   │   └── queue.svelte.ts
│       │   └── i18n/              # Translations (en, fr, es, zh, ru)
│       └── routes/                # SvelteKit pages
├── docker/                        # Dockerfiles + Nginx config
├── docs/                          # Documentation
├── .env.example                   # Environment template
└── docker-compose.yml             # Full-stack orchestration
```

## Backend Development

### Running with hot reload

For faster iteration, use `cargo-watch`:

```bash
cargo install cargo-watch
cd backend
cargo watch -x run
```

### Database migrations

Migrations run automatically on startup. To create a new migration:

```bash
cd backend/crates/soundtime-migration
# Edit src/ to add a new migration file
```

Migrations use Sea-ORM's `MigrationTrait`. See existing files in `soundtime-migration/src/` for examples.

### Code quality

```bash
# Format code
cargo fmt --all

# Run linter
cargo clippy --all-targets --all-features

# Run tests
cargo test --all
```

Both `cargo fmt` and `cargo clippy` must pass before submitting a PR.

### Logging

SoundTime uses the `tracing` crate. Control log levels with the `RUST_LOG` environment variable:

```bash
# All SoundTime logs at debug, everything else at info
RUST_LOG=info,soundtime=debug cargo run

# Verbose P2P logging
RUST_LOG=info,soundtime_p2p=trace cargo run

# SQL query logging
RUST_LOG=info,sea_orm=debug cargo run
```

## Frontend Development

### Development server

```bash
cd frontend
npm install
npm run dev
```

Vite's dev server provides hot module replacement (HMR) for instant feedback.

### Building for production

```bash
npm run build
npm run preview  # Preview the production build locally
```

### Adding translations

1. Copy an existing translation file in `src/lib/i18n/` (e.g., `en.ts`)
2. Translate all strings
3. Register the new locale in the i18n configuration
4. Submit a PR

### UI Components

SoundTime uses [shadcn-svelte](https://www.shadcn-svelte.com/) with Tailwind CSS. Components live in `src/lib/components/`.

## Docker Development

### Full stack with Docker Compose

```bash
docker compose up --build
```

Services:
- **Frontend**: http://localhost:3000
- **Backend API**: http://localhost:8080
- **Nginx proxy**: http://localhost:8880

### Rebuild a single service

```bash
docker compose build backend
docker compose up -d backend
```

### View logs

```bash
docker compose logs -f backend
docker compose logs -f frontend
```

### Database shell

```bash
docker compose exec postgres psql -U soundtime -d soundtime
```

## Environment Variables

See [`.env.example`](../.env.example) for all available variables. Key development settings:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgres://soundtime:soundtime@localhost:5432/soundtime` | PostgreSQL connection string |
| `JWT_SECRET` | — | Secret for signing JWTs (required) |
| `RUST_LOG` | `info` | Log level filter |
| `AUDIO_STORAGE_PATH` | `./data/music` | Where audio files are stored |
| `P2P_ENABLED` | `true` | Enable P2P node |
| `P2P_PORT` | `11204` | iroh QUIC port |
| `P2P_LOCAL_DISCOVERY` | `true` | Enable mDNS local discovery |
| `P2P_SEED_PEERS` | — | Comma-separated NodeIds to auto-connect |
| `CORS_ORIGINS` | — | Comma-separated allowed origins |
| `STORAGE_BACKEND` | `local` | `local` or `s3` |

## Troubleshooting

### Backend won't start

- **Database connection failed**: Ensure PostgreSQL is running (`docker compose up postgres -d`)
- **Port already in use**: Change `PORT` in `.env` or kill the process on port 8080
- **JWT_SECRET panic**: Only happens when `SOUNDTIME_ENV=production` with the default secret — use a random string

### Frontend can't reach API

- Ensure the backend is running on port 8080
- Check that `PUBLIC_API_URL` is set correctly if using a non-default port
- Check browser console for CORS errors — add your frontend URL to `CORS_ORIGINS`

### P2P not connecting

- Check that `P2P_ENABLED=true`
- Ensure UDP port 11204 is not blocked by firewall
- Check logs: `RUST_LOG=info,soundtime_p2p=debug cargo run`
- See the [P2P Networking Guide](p2p-networking.md) for detailed troubleshooting
