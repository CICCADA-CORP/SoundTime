# API Reference

SoundTime exposes a RESTful API over HTTP. All endpoints are prefixed with `/api` unless otherwise noted.

## Base URL

```
http://localhost:8080/api
```

In production behind Nginx, API requests are proxied from `https://your-domain.com/api`.

## Authentication

Most endpoints require a JWT access token in the `Authorization` header:

```
Authorization: Bearer <access_token>
```

Tokens are obtained via the [login](#post-apiauthlogin) or [refresh](#post-apiauthrefresh) endpoints.

| Token Type | Lifetime | Usage |
|-----------|----------|-------|
| Access token | 15 minutes | `Authorization: Bearer` header |
| Refresh token | 7 days | POST body to `/api/auth/refresh` |

### Visibility Modes

Some endpoints are conditionally authenticated based on instance settings:

- **Public instance** — tracks, albums, artists, playlists, and search are accessible without auth.
- **Private instance** — all content endpoints require a valid JWT.

---

## Health Check

### `GET /healthz`

Returns server health status. No authentication required.

**Response** `200 OK`
```json
{
  "status": "ok",
  "version": "0.1.0"
}
```

---

## Auth

All auth endpoints are **rate-limited** to 10 requests per 60 seconds per IP.

### `POST /api/auth/register`

Register a new user account. The first registered user automatically becomes the admin.

**Body** `application/json`
```json
{
  "username": "alice",
  "email": "alice@example.com",
  "password": "securepassword",
  "display_name": "Alice"
}
```

**Response** `201 Created`
```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ..."
}
```

### `POST /api/auth/login`

Authenticate with username and password.

**Body** `application/json`
```json
{
  "username": "alice",
  "password": "securepassword"
}
```

**Response** `200 OK`
```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ..."
}
```

### `POST /api/auth/refresh`

Exchange a valid refresh token for a new token pair.

**Body** `application/json`
```json
{
  "refresh_token": "eyJ..."
}
```

**Response** `200 OK`
```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ..."
}
```

### `GET /api/auth/me`

Get the current authenticated user's profile.

**Auth**: Required

**Response** `200 OK`
```json
{
  "id": "uuid",
  "username": "alice",
  "email": "alice@example.com",
  "display_name": "Alice",
  "role": "admin",
  "created_at": "2025-01-01T00:00:00Z"
}
```

### `PUT /api/auth/email`

Change the authenticated user's email address.

**Auth**: Required

**Body** `application/json`
```json
{
  "new_email": "newalice@example.com",
  "password": "current_password"
}
```

### `PUT /api/auth/password`

Change the authenticated user's password (minimum 8 characters).

**Auth**: Required

**Body** `application/json`
```json
{
  "current_password": "old_password",
  "new_password": "new_secure_password"
}
```

### `DELETE /api/auth/account`

Permanently delete the authenticated user's account and all associated data (GDPR-compliant). Cannot delete the last admin account.

**Auth**: Required

**Body** `application/json`
```json
{
  "password": "current_password"
}
```

---

## Setup

These endpoints are used during initial instance configuration. They are always accessible (never gated by private mode).

### `GET /api/setup/status`

Check whether the instance has completed initial setup.

**Response** `200 OK`
```json
{
  "setup_completed": false,
  "has_admin": false
}
```

### `POST /api/setup/admin`

Create the first admin user during initial setup.

**Body** `application/json`
```json
{
  "username": "admin",
  "email": "admin@example.com",
  "password": "securepassword",
  "display_name": "Admin"
}
```

### `POST /api/setup/instance`

Configure instance settings during setup.

**Auth**: Required

**Body** `application/json`
```json
{
  "instance_name": "My SoundTime",
  "description": "A personal music server",
  "private": false
}
```

### `POST /api/setup/complete`

Mark the setup wizard as complete.

**Auth**: Required

---

## Tracks

### `GET /api/tracks`

List all tracks with pagination.

**Auth**: Conditional (required if instance is private)

**Query Parameters**
| Parameter | Type | Description |
|-----------|------|-------------|
| `page` | integer | Page number (default: 1) |
| `per_page` | integer | Items per page (default: 20) |

**Response** `200 OK`
```json
{
  "tracks": [
    {
      "id": "uuid",
      "title": "Song Title",
      "artist_name": "Artist",
      "album_title": "Album",
      "duration": 240,
      "format": "flac",
      "file_size": 30000000,
      "cover_url": "/api/media/covers/...",
      "created_at": "2025-01-01T00:00:00Z"
    }
  ],
  "total": 150,
  "page": 1,
  "per_page": 20
}
```

### `GET /api/tracks/popular`

List tracks sorted by play count.

**Auth**: Conditional

### `GET /api/tracks/{id}`

Get a single track's full metadata.

**Auth**: Conditional

### `GET /api/tracks/{id}/credits`

Get track credits and contributors.

**Auth**: Conditional

### `GET /api/tracks/{id}/stream`

Stream the audio file. Returns the audio binary with appropriate `Content-Type` header.

**Auth**: Conditional

### `GET /api/tracks/{id}/lyrics`

Fetch lyrics for a track (from Musixmatch, Lyrics.com, or embedded metadata).

**Auth**: Conditional

### `GET /api/tracks/my-uploads`

List tracks uploaded by the current user.

**Auth**: Required

### `PUT /api/tracks/{id}`

Update a track's metadata.

**Auth**: Required

**Body** `application/json`
```json
{
  "title": "Updated Title",
  "artist_name": "Updated Artist",
  "genre": "Electronic"
}
```

### `DELETE /api/tracks/{id}`

Delete a track and its associated audio file.

**Auth**: Required

---

## Upload

### `POST /api/upload`

Upload a single audio file. Metadata is automatically extracted from the file tags. Maximum body size: **500 MB**.

**Auth**: Required

**Body**: `multipart/form-data`
| Field | Type | Description |
|-------|------|-------------|
| `file` | file | Audio file (FLAC, MP3, OGG, WAV, etc.) |

### `POST /api/upload/batch`

Upload multiple audio files at once. Maximum body size: **500 MB**.

**Auth**: Required

**Body**: `multipart/form-data` with multiple `file` fields.

---

## Albums

### `GET /api/albums`

List all albums with pagination.

**Auth**: Conditional

### `GET /api/albums/{id}`

Get a single album with its tracks.

**Auth**: Conditional

### `POST /api/albums/{id}/cover`

Upload a cover image for an album. Maximum body size: **500 MB**.

**Auth**: Required

**Body**: `multipart/form-data`
| Field | Type | Description |
|-------|------|-------------|
| `cover` | file | Image file (JPEG, PNG, WebP) |

---

## Artists

### `GET /api/artists`

List all artists with pagination.

**Auth**: Conditional

### `GET /api/artists/{id}`

Get a single artist with their albums and tracks.

**Auth**: Conditional

---

## Playlists

### `GET /api/playlists`

List all playlists (public playlists + user's own private playlists).

**Auth**: Conditional

### `GET /api/playlists/{id}`

Get a single playlist with its tracks.

**Auth**: Conditional

### `POST /api/playlists`

Create a new playlist.

**Auth**: Required

**Body** `application/json`
```json
{
  "name": "Chill Vibes",
  "description": "Relaxing tracks",
  "is_public": true
}
```

### `PUT /api/playlists/{id}`

Update a playlist's name, description, or visibility.

**Auth**: Required

### `DELETE /api/playlists/{id}`

Delete a playlist.

**Auth**: Required

### `POST /api/playlists/{id}/tracks`

Add a track to a playlist.

**Auth**: Required

**Body** `application/json`
```json
{
  "track_id": "uuid"
}
```

### `DELETE /api/playlists/{id}/tracks/{track_id}`

Remove a track from a playlist.

**Auth**: Required

---

## Favorites

### `GET /api/favorites`

List the authenticated user's favorite tracks.

**Auth**: Required

### `GET /api/favorites/check`

Check if specific tracks are in the user's favorites.

**Auth**: Required

**Query Parameters**
| Parameter | Type | Description |
|-----------|------|-------------|
| `track_ids` | string | Comma-separated track UUIDs |

### `POST /api/favorites/{track_id}`

Add a track to favorites.

**Auth**: Required

### `DELETE /api/favorites/{track_id}`

Remove a track from favorites.

**Auth**: Required

---

## History

### `GET /api/history`

List the authenticated user's listening history.

**Auth**: Required

### `POST /api/history`

Log a listen event.

**Auth**: Required

**Body** `application/json`
```json
{
  "track_id": "uuid"
}
```

---

## Libraries

### `GET /api/libraries`

List available libraries.

**Auth**: Conditional

### `GET /api/libraries/{id}`

Get a single library.

**Auth**: Conditional

---

## Search

### `GET /api/search`

Full-text search across tracks, albums, and artists.

**Auth**: Conditional

**Query Parameters**
| Parameter | Type | Description |
|-----------|------|-------------|
| `q` | string | Search query |

**Response** `200 OK`
```json
{
  "tracks": [...],
  "albums": [...],
  "artists": [...]
}
```

---

## Editorial Playlists

### `GET /api/editorial-playlists`

List AI-generated editorial playlists.

**Auth**: Conditional

---

## Users

### `GET /api/users/{id}`

Get a user's public profile.

**Auth**: Conditional

---

## Reports

### `POST /api/tracks/{id}/report`

Report a track for moderation.

**Auth**: Required

**Body** `application/json`
```json
{
  "reason": "Copyright violation"
}
```

---

## Media

### `GET /api/media/{*path}`

Serve static media files (cover art, waveforms, etc.).

**Auth**: Conditional

---

## Terms of Service

### `GET /api/tos`

Get the instance's Terms of Service.

**Auth**: Conditional

---

## P2P

### `GET /api/p2p/status`

Get the P2P node status.

**Response** `200 OK`
```json
{
  "enabled": true,
  "node_id": "abcdef1234...",
  "relay_url": "https://relay.example.com",
  "peers_count": 3
}
```

### `GET /api/p2p/network-graph`

Get the P2P network topology for visualization (used by the D3.js network graph).

**Response** `200 OK`
```json
{
  "nodes": [
    { "id": "node_id_1", "label": "My Instance" }
  ],
  "links": [
    { "source": "node_id_1", "target": "node_id_2" }
  ]
}
```

---

## Admin

All admin endpoints require the `admin` role. The role is verified from the database on each request (not just from the JWT claim).

### Dashboard

#### `GET /api/admin/stats`

Get dashboard statistics.

**Response** `200 OK`
```json
{
  "total_users": 42,
  "total_tracks": 1500,
  "total_albums": 200,
  "total_artists": 150,
  "storage_used": "12.5 GB"
}
```

### Settings

#### `GET /api/admin/settings`

Get all instance settings (instance name, private mode, registration, etc.).

#### `PUT /api/admin/settings/{key}`

Update a single instance setting.

**Body** `application/json`
```json
{
  "value": "new_value"
}
```

### User Management

#### `GET /api/admin/users`

List all registered users with roles and status.

#### `PUT /api/admin/users/{id}/role`

Change a user's role between `user` and `admin`.

**Body** `application/json`
```json
{
  "role": "admin"
}
```

#### `PUT /api/admin/users/{id}/ban`

Ban a user (revokes all tokens, prevents login).

#### `DELETE /api/admin/users/{id}/ban`

Unban a user.

### Content Moderation

#### `GET /api/admin/reports`

List all content reports.

#### `GET /api/admin/reports/stats`

Get report statistics (pending, resolved, total).

#### `PUT /api/admin/reports/{id}`

Resolve a report (approve, dismiss, or remove content).

#### `GET /api/admin/tracks/browse`

Browse all tracks for moderation purposes.

#### `DELETE /api/admin/tracks/{id}/moderate`

Delete a track through moderation (with logging).

### Terms of Service

#### `PUT /api/admin/tos`

Update the Terms of Service content.

#### `DELETE /api/admin/tos`

Reset Terms of Service to default.

### Blocked Domains / Peers

#### `GET /api/admin/blocked-domains`

List blocked domains and P2P peers.

#### `POST /api/admin/blocked-domains`

Block a domain or P2P peer NodeId.

**Body** `application/json`
```json
{
  "domain": "malicious-peer-node-id"
}
```

#### `GET /api/admin/blocked-domains/export`

Export the blocklist as JSON.

#### `POST /api/admin/blocked-domains/import`

Import a blocklist from JSON.

#### `DELETE /api/admin/blocked-domains/{id}`

Remove a domain/peer from the blocklist.

### Federation / Instances

#### `GET /api/admin/instances`

List known federated/P2P instances.

#### `POST /api/admin/instances/health-check`

Run health checks on all known instances.

### Metadata Enrichment

#### `GET /api/admin/metadata/status`

Get metadata enrichment status (MusicBrainz integration).

#### `POST /api/admin/metadata/enrich/{track_id}`

Enrich a single track's metadata from MusicBrainz.

#### `POST /api/admin/metadata/enrich-all`

Enrich metadata for all tracks.

### Editorial Playlists

#### `GET /api/admin/editorial/status`

Check editorial playlist generation status.

#### `POST /api/admin/editorial/generate`

Manually trigger AI editorial playlist generation.

### Remote Tracks (P2P)

#### `GET /api/admin/remote-tracks`

List tracks replicated from remote P2P peers.

### Storage

#### `GET /api/admin/storage/status`

Get storage backend status and statistics.

#### `POST /api/admin/storage/integrity-check`

Run a storage integrity check (verify all files exist and match database records).

#### `POST /api/admin/storage/sync`

Trigger a storage sync/import operation.

### P2P Peer Management

#### `GET /api/admin/p2p/peers`

List all connected and known P2P peers.

#### `POST /api/admin/p2p/peers`

Manually add a P2P peer by NodeId.

**Body** `application/json`
```json
{
  "node_id": "abcdef1234567890..."
}
```

#### `DELETE /api/admin/p2p/peers/{node_id}`

Remove a P2P peer.

#### `POST /api/admin/p2p/peers/{node_id}/ping`

Ping a specific P2P peer to check connectivity.

---

## Error Responses

All error responses follow a consistent format:

```json
{
  "error": "Human-readable error message"
}
```

| Status Code | Meaning |
|------------|---------|
| 400 | Bad Request — invalid input or missing fields |
| 401 | Unauthorized — missing or invalid JWT |
| 403 | Forbidden — insufficient permissions |
| 404 | Not Found — resource does not exist |
| 409 | Conflict — duplicate resource (e.g., username taken) |
| 413 | Payload Too Large — file exceeds 500 MB limit |
| 429 | Too Many Requests — rate limit exceeded |
| 500 | Internal Server Error |

## Rate Limiting

Auth endpoints (`/api/auth/*`) are rate-limited to **10 requests per 60 seconds** per IP address via `tower-governor`. When exceeded, the server responds with `429 Too Many Requests`.
