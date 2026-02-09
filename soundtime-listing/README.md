# SoundTime Listing — Public Node Directory

A lightweight service that maintains a public directory of SoundTime instances.
Nodes self-register via heartbeat, are health-checked every 5 minutes, and automatically removed after 48 hours of downtime.

## Quick Start

```bash
npm install
npm start
```

The server runs on `http://localhost:3333` by default.

## Environment Variables

| Variable  | Default               | Description             |
|-----------|-----------------------|-------------------------|
| `PORT`    | `3333`                | HTTP server port        |
| `DB_PATH` | `./data/listing.db`   | SQLite database path    |

## API

### `POST /api/announce` — Register / Heartbeat

Register a new node or send a heartbeat for an existing one.

**Request body:**
```json
{
  "domain": "music.example.com",
  "name": "My SoundTime Instance",
  "description": "A community music server",
  "version": "0.1.0",
  "token": null
}
```

- Omit `token` for first registration → response includes a token to save.
- Include `token` for heartbeat updates.

**Response (first registration):**
```json
{
  "status": "registered",
  "id": "uuid",
  "domain": "music.example.com",
  "token": "save-this-token",
  "message": "Save this token! You need it for future heartbeats and to remove your node."
}
```

### `GET /api/nodes` — List nodes

Returns all online nodes. Add `?include_offline=true` for all nodes.

### `GET /api/nodes/:domain` — Node details

### `DELETE /api/nodes/:domain` — Remove node

Requires `Authorization: Bearer <token>` header.

### `GET /api/stats` — Aggregate statistics

## Docker

```bash
docker build -t soundtime-listing .
docker run -d -p 3333:3333 -v listing-data:/app/data soundtime-listing
```

## How It Works

1. **Registration**: SoundTime instances call `POST /api/announce` with their domain. The listing server verifies the node is reachable via `/healthz` before accepting.

2. **Heartbeat**: Registered nodes periodically send heartbeats (same endpoint, with their token). The listing server also fetches fresh stats from `/api/nodeinfo`.

3. **Health Checks**: Every 5 minutes, the server checks all registered nodes via their `/healthz` endpoint.

4. **Auto-Removal**: Nodes that have been offline for more than 48 hours are automatically removed from the directory.

## Integration with SoundTime

In your SoundTime admin panel, enable "Publish to Public Directory" to automatically register and send heartbeats to the listing server.

## License

AGPL-3.0 — Same as SoundTime
