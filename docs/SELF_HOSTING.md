# Self-Hosting Guide

Run your own SoundTime node with Docker Compose.

## Prerequisites

- **Docker** and **Docker Compose** installed.
- access to ports 80/443 (for web) and 11204/udp (for P2P).
- A domain name (optional, but recommended for SSL).

## Quick Start

1. **Clone the repository**
   ```bash
   git clone https://github.com/CICCADA-CORP/SoundTime.git
   cd SoundTime
   ```

2. **Configure Environment**
   Copy the example environment file:
   ```bash
   cp .env.example .env
   ```
   
   Edit `.env` and set secure passwords:
   ```ini
   POSTGRES_PASSWORD=replace_with_secure_password
   JWT_SECRET=generate_a_long_random_string_here
   DOMAIN=soundtime.local
   ```

3. **Start Services**
   ```bash
   docker compose up -d
   ```

4. **Access**
   Open your browser and navigate to `http://localhost:3000` (or your configured domain).

## Docker Compose Structure

Our `docker-compose.yml` orchestrates three main services:

| Service | Description | Internal Port |
| ------- | ----------- | ------------- |
| `app` | The main Rust binary (backend). | 8080 |
| `web` | The compiled SvelteKit frontend (Node.js adapter). | 3000 |
| `db` | PostgreSQL database. | 5432 |
| `proxy` | Nginx reverse proxy (handles SSL & routing). | 80/443 |

## Updating

To update your instance to the latest version:

```bash
docker compose pull
docker compose up -d
docker image prune -f
```

## Troubleshooting

**Logs**:
```bash
docker compose logs -f app
```

**Database Shell**:
```bash
docker compose exec db psql -U soundtime -d soundtime_db
```
