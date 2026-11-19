# Self-Hosting Guide

Run your own SoundTime node with Docker Compose. For guided installation on a computer, NAS or remote server, see the [installer guide](installateur.md).

## Prerequisites

- **Docker** and **Docker Compose** installed.
- Access to the web proxy port (8880 by default) and 11204/UDP for P2P.
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
   
   Set a new database password in `.env`. Run `openssl rand -base64 32` and paste its output into `JWT_SECRET`. Set your public hostname, if available.
   ```ini
   POSTGRES_PASSWORD=replace_with_secure_password
   JWT_SECRET=generate_a_long_random_string_here
   SOUNDTIME_DOMAIN=soundtime.local
   ```

   To import music from a NAS, enable the SMB or NFS Compose file in `.env`. [Network library setup](deployment.md#read-only-smb-and-nfs-music-libraries)

3. **Start Services**
   ```bash
   docker compose up -d
   ```

4. **Access**
   Open the web proxy at `http://localhost:8880`, or use the address and `NGINX_PORT` configured for your server.

## Docker Compose Structure

The default Compose file starts five services.

| Service | Description | Internal Port |
| ------- | ----------- | ------------- |
| `backend` | Rust API and P2P node | 8080, 11204/UDP |
| `frontend` | SvelteKit frontend | 3000 |
| `postgres` | PostgreSQL database with pgvector | 5432 |
| `redis` | Redis cache | 6379 |
| `nginx` | HTTP reverse proxy | 80 |

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
docker compose logs -f backend
```

**Database Shell**:
```bash
docker compose exec postgres sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
```
