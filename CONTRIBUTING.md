# Contributing to SoundTime

Thank you for your interest in contributing to SoundTime! This guide will help you get started, whether you're fixing a bug, adding a feature, improving documentation, or helping with translations.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Making Changes](#making-changes)
- [Code Style](#code-style)
- [Commit Convention](#commit-convention)
- [Pull Request Process](#pull-request-process)
- [Adding Translations](#adding-translations)
- [Good First Issues](#good-first-issues)
- [Need Help?](#need-help)

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold a welcoming, inclusive environment.

## Getting Started

1. **Fork** the repository on GitHub
2. **Clone** your fork locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/SoundTime.git
   cd SoundTime
   ```
3. **Add upstream** remote:
   ```bash
   git remote add upstream https://github.com/CICCADA-CORP/SoundTime.git
   ```
4. **Create a branch** for your work:
   ```bash
   git checkout -b feat/my-awesome-feature
   ```

## Development Setup

### Prerequisites

| Tool | Version | Installation |
|------|---------|-------------|
| Rust | 1.78+ | [rustup.rs](https://rustup.rs) |
| Node.js | 20+ | [nvm](https://github.com/nvm-sh/nvm) or [nodejs.org](https://nodejs.org) |
| PostgreSQL | 16 | Via Docker (recommended) or native install |
| Docker | Latest | [docker.com](https://docs.docker.com/get-docker/) |

### Quick Setup

```bash
# 1. Start PostgreSQL
docker compose up postgres -d

# 2. Configure environment
cp .env.example .env
# Edit .env with your local settings

# 3. Run the backend
cd backend
cargo run
# Migrations run automatically on startup

# 4. Run the frontend (new terminal)
cd frontend
npm install
npm run dev
```

The backend starts at `http://localhost:8080` and the frontend at `http://localhost:5173`.

### Running Tests

```bash
# Backend tests (all crates)
cd backend
cargo test --all

# Frontend unit tests
cd frontend
npm run test

# Frontend E2E tests (requires backend running)
cd frontend
npm run cy:run

# Type checking
cd frontend
npx svelte-check --tsconfig ./tsconfig.json
```

### Full Docker Setup

If you prefer to run everything in Docker:

```bash
docker compose up --build
```

## Project Structure

```
soundtime/
├── backend/crates/
│   ├── soundtime-server/    # HTTP server, routes, auth, middleware
│   ├── soundtime-db/        # Sea-ORM entities, DB connection pool
│   ├── soundtime-migration/ # Database migrations
│   ├── soundtime-audio/     # Audio processing, storage, waveform
│   └── soundtime-p2p/       # P2P networking (iroh, blobs, peers)
├── frontend/src/
│   ├── lib/components/      # Reusable UI components
│   ├── lib/stores/          # Svelte 5 rune stores
│   ├── lib/i18n/            # Internationalization (5 languages)
│   ├── lib/api.ts           # API client
│   └── routes/              # SvelteKit pages
├── docker/                  # Dockerfiles + Nginx config
└── docs/                    # Documentation
```

## Making Changes

### Backend (Rust)

1. All backend code lives in `backend/crates/`
2. Changes to the API go in `soundtime-server/src/api/`
3. New database fields need a migration in `soundtime-migration/`
4. New entities go in `soundtime-db/src/entity/`

### Frontend (SvelteKit)

1. Pages are in `frontend/src/routes/`
2. Reusable components go in `frontend/src/lib/components/`
3. API types are defined in `frontend/src/lib/types.ts`
4. State management uses Svelte 5 runes (`$state`, `$derived`, `$effect`)

### Database Changes

If your change requires a database schema change:

1. Create a new migration in `backend/crates/soundtime-migration/src/`
2. Follow the existing naming pattern: `mNNNNNN_description.rs`
3. Register it in `lib.rs`
4. Migrations run automatically on server startup

## Code Style

### Rust (Backend)

- **Format**: Run `cargo fmt` before every commit
- **Lint**: Run `cargo clippy` and fix all warnings
- **Logging**: Use `tracing` macros (`info!`, `warn!`, `debug!`) — not `println!`
- **Error handling**: Use `Result` types with descriptive errors, avoid `.unwrap()` in production code
- **Tests**: Write tests for new functionality, use `#[tokio::test]` for async tests
- **Imports**: Group by std → external crates → internal crates, separated by blank lines

### TypeScript/Svelte (Frontend)

- **TypeScript**: Strict mode — no `any` types unless absolutely necessary
- **Svelte 5**: Use runes pattern (`$state`, `$derived`, `$props`) — not legacy `let` reactivity
- **Styling**: Use Tailwind CSS utilities only — avoid custom CSS files
- **Components**: Follow existing patterns in `lib/components/`
- **Types**: Define API response types in `lib/types.ts`

### General

- Keep functions focused and small (< 50 lines when possible)
- Write descriptive variable and function names
- Comment "why", not "what" — the code should explain what it does

## Commit Convention

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

| Type | Description |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `style` | Formatting, no code change |
| `refactor` | Code restructuring, no behavior change |
| `test` | Adding or updating tests |
| `chore` | Build, CI, tooling changes |
| `perf` | Performance improvement |

### Scopes

Use the module name: `backend`, `frontend`, `p2p`, `audio`, `db`, `docker`, `ci`, `i18n`

### Examples

```
feat(frontend): add waveform zoom controls
fix(backend): handle empty album art gracefully
docs: update deployment guide for S3 storage
test(p2p): add relay connection timeout tests
chore(ci): cache cargo registry in GitHub Actions
```

## Pull Request Process

1. **Sync with upstream** before opening a PR:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Ensure all checks pass locally**:
   ```bash
   # Backend
   cd backend && cargo fmt --check && cargo clippy && cargo test --all

   # Frontend
   cd frontend && npx svelte-check && npm run test && npm run build
   ```

3. **Open a Pull Request** using the [PR template](.github/PULL_REQUEST_TEMPLATE.md)

4. **Address review feedback** — maintainers may request changes

5. **Squash and merge** — we use squash merging to keep a clean history

### PR Checklist

- [ ] Branch is up to date with `main`
- [ ] `cargo fmt` and `cargo clippy` pass
- [ ] `svelte-check` and `npm run build` pass
- [ ] New code has tests
- [ ] Documentation is updated if needed
- [ ] Translations are added for new UI strings (at minimum `en.ts`)

## Adding Translations

SoundTime supports 5 languages. Translation files are in `frontend/src/lib/i18n/translations/`:

| File | Language |
|------|----------|
| `en.ts` | English (primary) |
| `fr.ts` | Français |
| `es.ts` | Español |
| `zh.ts` | 中文 |
| `ru.ts` | Русский |

### Adding a String

1. Add the key to `en.ts` first (English is the source of truth)
2. Add translations to all other language files
3. Use the key in your component: `{t('section.key')}`

### Adding a New Language

1. Create a new file (e.g., `ja.ts`) based on `en.ts`
2. Translate all keys
3. Register it in `frontend/src/lib/i18n/index.svelte.ts`
4. Open a PR — we'd love to support more languages!

## Good First Issues

New to the project? Look for issues labeled [`good first issue`](https://github.com/CICCADA-CORP/SoundTime/labels/good%20first%20issue). These are beginner-friendly tasks that help you get familiar with the codebase.

Some ideas for first contributions:
- 🌍 **Translate** — Add a new language or improve existing translations
- 📝 **Documentation** — Fix typos, improve guides, add examples
- 🎨 **UI polish** — Improve responsive design, fix styling issues
- 🧪 **Tests** — Add missing test coverage for existing features

## Need Help?

- 💬 [GitHub Discussions](https://github.com/CICCADA-CORP/SoundTime/discussions) — Ask questions, share ideas
- 📋 [GitHub Issues](https://github.com/CICCADA-CORP/SoundTime/issues) — Report bugs, request features
- 📖 [Documentation](docs/) — Architecture, API reference, deployment guides

---

Thank you for helping make SoundTime better! 🎵
