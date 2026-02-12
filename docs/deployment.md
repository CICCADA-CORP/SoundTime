# Deployment Guide

Deploy SoundTime in production with Docker Compose, SSL, and optional S3 storage.

## Requirements

- A Linux server (Ubuntu 22.04+, Debian 12+, or similar)
- **Docker** and **Docker Compose** v2
- A domain name (recommended for SSL)
- At least **1 GB RAM** and **10 GB disk** (scales with library size)
- Ports **80/443** (web) and **11204/UDP** (P2P) open

## Quick Deploy

```bash
# Clone the repository
git clone https://github.com/CICCADA-CORP/SoundTime.git
cd SoundTime

# Configure environment
cp .env.example .env
nano .env  # Edit with your production values

# Start all services
docker compose up --build -d
```

## Environment Configuration

Edit `.env` with production values. **All variables marked as required must be set.**

### Core Settings

```env
# ─── Required ───
JWT_SECRET=<generate with: openssl rand -base64 32>
POSTGRES_PASSWORD=<strong random password>
DOMAIN=music.example.com

# ─── Application ───
HOST=0.0.0.0
PORT=8080
SOUNDTIME_ENV=production
RUST_LOG=info,soundtime=info
```

> **Warning**: The server will **panic** on startup if `JWT_SECRET` is left as the default value when `SOUNDTIME_ENV=production`.

### Database

```env
DATABASE_URL=postgres://soundtime:<your_password>@postgres:5432/soundtime
POSTGRES_USER=soundtime
POSTGRES_PASSWORD=<same strong password>
POSTGRES_DB=soundtime
```

### Storage

#### Local filesystem (default)

```env
STORAGE_BACKEND=local
AUDIO_STORAGE_PATH=/data/music
```

#### S3-compatible storage (MinIO, AWS S3, etc.)

```env
STORAGE_BACKEND=s3
S3_ENDPOINT=https://s3.amazonaws.com    # or your MinIO URL
S3_REGION=us-east-1
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
S3_BUCKET=soundtime-music
S3_PREFIX=audio/                         # optional key prefix
S3_CACHE_PATH=/tmp/soundtime-s3-cache    # local cache for streaming
```

### P2P Networking

```env
P2P_ENABLED=true
P2P_PORT=11204
P2P_BIND_PORT=11204
P2P_BLOBS_DIR=/data/p2p/blobs
P2P_SECRET_KEY_PATH=/data/p2p/secret_key
P2P_LOCAL_DISCOVERY=false               # disable mDNS in production
P2P_SEED_PEERS=                         # comma-separated NodeIds of peers to auto-connect
P2P_CACHE_MAX_SIZE=2GB                  # max disk for cached P2P blobs (default: 2GB)
```

> **Important**: Open UDP port **11204** in your firewall for P2P connectivity. If behind NAT, SoundTime will use n0.computer relay servers as fallback.

> **P2P Cache**: Remote tracks are fetched on-demand when played and cached locally. The `P2P_CACHE_MAX_SIZE` setting controls the maximum disk space for cached blobs. When the limit is reached, least-recently-played tracks are evicted. Accepts values like `512MB`, `2GB`, `5GB`, `1TB`, or raw byte counts. Default is `2GB`.

### Public Instance Listing

```env
# Controlled via admin settings panel:
# listing_public = true/false          # opt-in to the SoundTime public directory
# listing_url = <custom URL>           # override the default listing server
# listing_domain = <your domain>       # domain announced to the directory
```

The listing worker sends a heartbeat every 5 minutes to the SoundTime public directory. Listing is **opt-in** — new instances are not listed by default. Enable it from the admin panel by setting `listing_public` to `true`. When disabled, a DELETE request is sent immediately to remove the instance from the directory. As a fallback, the directory server also removes instances that haven't sent a heartbeat for 48 hours.

### CORS & Security

```env
CORS_ORIGINS=https://music.example.com
SOUNDTIME_SCHEME=https
SOUNDTIME_DOMAIN=music.example.com
```

## Docker Compose Architecture

The `docker-compose.yml` file orchestrates four services:

| Service | Image | Ports | Description |
|---------|-------|-------|-------------|
| **postgres** | `postgres:16` | internal | PostgreSQL database with health checks |
| **backend** | Built from `docker/Dockerfile.backend` | `8080`, `11204/udp` | Rust Axum API server |
| **frontend** | Built from `docker/Dockerfile.frontend` | `3000` | SvelteKit SSR server |
| **nginx** | `nginx:alpine` | `80` (or `NGINX_PORT`) | Reverse proxy |

### Persistent Volumes

| Volume | Mount Point | Description |
|--------|-------------|-------------|
| `postgres_data` | `/var/lib/postgresql/data` | Database files |
| `music_data` | `/data/music` | Uploaded audio, covers, waveforms |
| `p2p_data` | `/data/p2p-blobs` | iroh-blobs store & secret key |

## SSL / TLS with Let's Encrypt

### Option 1: Certbot + Nginx (recommended)

1. Install Certbot on your host:
   ```bash
   sudo apt install certbot
   ```

2. Obtain a certificate:
   ```bash
   sudo certbot certonly --standalone -d music.example.com
   ```

3. Update the Nginx config in `docker/nginx.conf` to include SSL:
   ```nginx
   server {
       listen 443 ssl;
       server_name music.example.com;

       ssl_certificate /etc/letsencrypt/live/music.example.com/fullchain.pem;
       ssl_certificate_key /etc/letsencrypt/live/music.example.com/privkey.pem;

       # ... existing proxy rules ...
   }

   server {
       listen 80;
       server_name music.example.com;
       return 301 https://$host$request_uri;
   }
   ```

4. Mount the certificates into the Nginx container by adding to `docker-compose.yml`:
   ```yaml
   nginx:
     volumes:
       - /etc/letsencrypt:/etc/letsencrypt:ro
   ```

5. Set up auto-renewal:
   ```bash
   sudo certbot renew --dry-run
   ```

### Option 2: External reverse proxy

If you already have a reverse proxy (Traefik, Caddy, etc.), point it to the Nginx service or directly to the backend/frontend ports.

## Custom Domain

1. Set your domain's DNS A record to your server's IP address
2. Update `.env`:
   ```env
   DOMAIN=music.example.com
   SOUNDTIME_SCHEME=https
   SOUNDTIME_DOMAIN=music.example.com
   CORS_ORIGINS=https://music.example.com
   ```
3. Restart services:
   ```bash
   docker compose down && docker compose up -d
   ```

## Maintenance

### Updating

```bash
cd SoundTime
git pull
docker compose down
docker compose up --build -d
docker image prune -f
```

Database migrations run automatically on backend startup.

### Backups

#### Database

```bash
# Export database
docker compose exec postgres pg_dump -U soundtime soundtime > backup.sql

# Restore database
cat backup.sql | docker compose exec -T postgres psql -U soundtime soundtime
```

#### Audio files

Back up the `music_data` volume or the `AUDIO_STORAGE_PATH` directory:

```bash
# Find the volume location
docker volume inspect soundtime_music_data

# Or tar the data directory
tar czf music-backup.tar.gz /path/to/data/music
```

#### P2P identity

Back up `p2p_data` to preserve your NodeId across migrations:

```bash
docker volume inspect soundtime_p2p_data
```

### Logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f backend

# Last 100 lines
docker compose logs --tail 100 backend
```

### Database shell

```bash
docker compose exec postgres psql -U soundtime -d soundtime
```

## Monitoring

### Health check

The backend exposes a health endpoint:

```bash
curl http://localhost:8080/healthz
# {"status":"ok","version":"0.1.0"}
```

### P2P status

```bash
curl http://localhost:8080/api/p2p/status
```

## Performance Tuning

### PostgreSQL

For larger installations, tune PostgreSQL in `docker-compose.yml`:

```yaml
postgres:
  command: >
    postgres
    -c shared_buffers=256MB
    -c effective_cache_size=768MB
    -c maintenance_work_mem=128MB
    -c max_connections=100
```

### Nginx

Increase upload timeouts for large files in `docker/nginx.conf`:

```nginx
client_max_body_size 500M;
proxy_read_timeout 300s;
proxy_send_timeout 300s;
```

## Troubleshooting

### Backend won't start

- Check logs: `docker compose logs backend`
- Ensure PostgreSQL is healthy: `docker compose ps`
- Verify `DATABASE_URL` matches your Postgres credentials
- Ensure `JWT_SECRET` is set to a non-default value

### P2P not connecting

- Verify UDP port 11204 is open: `sudo ufw allow 11204/udp`
- Check P2P logs: set `RUST_LOG=info,soundtime_p2p=debug`
- If behind NAT, relay servers are used automatically
- See [P2P Networking Guide](p2p-networking.md) for details

### Storage issues

- Check disk space: `df -h`
- Verify volume mounts: `docker compose exec backend ls -la /data/music`
- Run integrity check from the admin panel or API:
  ```bash
  curl -X POST http://localhost:8080/api/admin/storage/integrity-check \
    -H "Authorization: Bearer <admin_token>"
  ```

### CORS errors

Ensure `CORS_ORIGINS` includes your exact frontend URL (with scheme):
```env
CORS_ORIGINS=https://music.example.com
```
