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
| Media cookie | Up to 10 minutes | Native browser requests for track streams, radio relays and artwork |

### Visibility Modes

Some endpoints are conditionally authenticated based on instance settings:

- **Public instance** — tracks, albums, artists, playlists, and search are accessible without auth.
- **Private instance** — all content endpoints require a valid JWT.

---

## Webradios

The **Webradios** navigation entry opens `/webradios`. Each instance queries the external directory on demand and keeps bounded, short-lived pages in memory. Station listings and live broadcasts are separate from SoundTime tracks, storage quotas and federation. [Radio Browser API](https://docs.radio-browser.info/)

| Endpoint | Result |
| --- | --- |
| `GET /api/webradios/status` | Returns `instance_enabled`, `user_enabled` and their conjunction `enabled`. Both settings default to true. |
| `GET /api/webradios/stations` | Accepts optional `name`, `tag`, `offset` and `limit` (1–60, default 30). Returns `stations` and a nullable `next_offset`. Each station includes its UUID, name, icon when available, description or broadcast details, tags, country, language, codec, bitrate and HLS flag. |
| `POST /api/webradios/stations/{id}/play` | Resolves the selected station, reports the click to Radio Browser and returns `url`, `hls` and an optional `relay_url`. |
| `GET /api/webradios/stations/{id}/stream` | Relays compatible continuous HTTP audio by station UUID. SoundTime applies its streaming limits and checks the access setting during playback. |
| `PUT /api/webradios/preference` | Saves `{"enabled":false}` or `{"enabled":true}` for the signed-in listener. Guests can change their preference in the browser. |
| `PUT /api/admin/settings/webradios_enabled` | An administrator supplies `{"value":"false"}` or `{"value":"true"}` for the whole instance. |

Directory and playback endpoints follow instance privacy. A disabled directory returns HTTP 403 for list and play requests, including known station UUIDs. HTTPS broadcasters serve listeners directly and receive the listener’s IP. An HTTP-only continuous audio station can use the same-origin relay when the site uses HTTPS. An HTTP-only HLS playlist may require an external player. Broadcasters control availability, codec support and cross-origin access to HLS segments.

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

Register a new user account. The first registered user becomes the admin. After the first account exists, registration requires completed setup, a public instance and enabled registrations. An existing public instance with no `open_registrations` setting keeps its previous open-registration behavior. When registrations are closed the server returns `403 Forbidden`.

`GET /api/nodeinfo` exposes this same effective policy as the boolean `open_registration`.

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
  "user": {
    "id": "f7253772-aa17-49c0-a38b-89bd44711c85",
    "username": "alice",
    "email": "alice@example.com",
    "display_name": "Alice",
    "avatar_url": null,
    "role": "user",
    "instance_id": "alice@music.example"
  },
  "tokens": {
    "access_token": "eyJ...",
    "refresh_token": "eyJ...",
    "token_type": "Bearer",
    "expires_in": 900
  }
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

**Response** `200 OK` with the same `user` and `tokens` object as registration.

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

### `POST /api/auth/media-session`

Exchange a Bearer access token for media access on the same origin. A successful response contains `expires_in` in seconds and sets HttpOnly, SameSite=Strict cookies named `soundtime_media`, scoped to `/api/tracks/`, `/api/media/` and `/api/webradios/stations/`. HTTPS requests receive Secure cookies. The stream and artwork handlers accept the media credential for GET and HEAD.

The grant expires within 600 seconds and by the end of the access token's lifetime. When the access token has 45 seconds or less remaining, the exchange returns `401`. Refresh the access token and repeat the exchange before resuming playback.

### `DELETE /api/auth/media-session`

Expire the scoped media cookies on logout. The server returns `204 No Content`, including when the access token has expired.

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

Setup status is public. Instance configuration and completion require an authenticated administrator.

### `GET /api/setup/status`

Check whether the instance has completed initial setup.

**Response** `200 OK`
```json
{
  "setup_complete": false,
  "has_admin": false,
  "instance_private": false
}
```

### `POST /api/setup/admin`

Create the first admin user during initial setup.

**Body** `application/json`
```json
{
  "username": "admin",
  "email": "admin@example.com",
  "password": "securepassword"
}
```

On success, the `201 Created` response contains the admin profile and token pair in the same shape as `/api/auth/register`.

### `POST /api/setup/instance`

Configure instance settings during setup.

**Auth**: Required

**Body** `application/json`
```json
{
  "instance_name": "My SoundTime",
  "instance_description": "A personal music server"
}
```

### `POST /api/setup/complete`

Save the final instance settings and mark setup complete in one transaction. Send the access token returned by account creation or administrator login. The optional `instance_private` field sets visibility. Omitting it preserves the current instance visibility.

```json
{
  "p2p_enabled": false,
  "open_registrations": false,
  "max_upload_size_mb": 500,
  "instance_private": true
}
```

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

Get track credits and contributors. An uncredited track has a null `artist_name`. Clients can use the track ID to open and play it.

**Auth**: Conditional

### `GET /api/tracks/{id}/stream`

Stream the audio file with the appropriate `Content-Type`. On private instances, send a Bearer access token or a media cookie. Keep the track URL free of credentials. A satisfiable byte range such as `Range: bytes=64-191` returns `206 Partial Content` with `Content-Range`.

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

If an identical file is already present, the server returns HTTP 409. Re-uploading a known missing file restores the original track and its playlist references.

### `POST /api/upload/batch`

Upload up to 50 audio files at once. Each duplicate is reported as a failed item and other files continue to import. Maximum body size: **500 MB**.

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

List credited artists with pagination. The total, page count and search results exclude the placeholder used internally for uncredited tracks.

**Auth**: Conditional

### `GET /api/artists/{id}`

Get a single artist with their albums and tracks. A request for the internal placeholder returns HTTP 404.

**Auth**: Conditional

---

## Playlists

### `GET /api/playlists`

Discover public playlists, ordered by last update. On a private instance this endpoint requires authentication.

**Auth**: Conditional

### `GET /api/me/playlists`

List the authenticated member’s public and private playlists, with track counts. Accepts `page` (default 1) and `per_page` (default 20, maximum 100). Returns `data`, `page`, `per_page`, `total` and `total_pages`.

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

Repeating a successful addition returns HTTP 204 and preserves the existing position.

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

Serve JPEG, PNG, GIF and WebP cover images from the media directories. The response uses the image format detected from the file contents. Other file types return HTTP 404.

Private instances accept a Bearer access token or a media cookie.

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
  "relay_connected": true,
  "direct_addresses": 2,
  "peer_count": 3,
  "online_peer_count": 2,
  "dht_discovery_enabled": true,
  "rendezvous_configured": false
}
```

### `POST /api/p2p/rendezvous`

Instances with `P2P_RENDEZVOUS_SERVER=true` expose this endpoint for participating P2P nodes. Send a JSON object with the lowercase hexadecimal `node_id`, a Unix `timestamp` in seconds, and a `signature` array containing the 64 Ed25519 signature bytes. Sign the UTF-8 string `soundtime/rendezvous/1/{node_id}/{timestamp}`.

A successful response contains a JSON array of up to eight other active NodeIds. Registrations expire 15 minutes after the signed timestamp. Invalid signatures and expired timestamps return `400`. A full directory returns `503` for a new participant.

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

Look up a local recording using its embedded recording MBID, ISRC, or title and performer. Album, duration, credits and version distinguish candidates. A confident result updates the existing track. An ambiguous result returns up to five candidates with recording ID, performer, album, duration and disambiguation when available. Confirm a candidate with `?recording_id={mbid}` on the same endpoint. A failed lookup leaves the existing metadata and playlist references intact. [MusicBrainz search fields](https://musicbrainz.org/doc/MusicBrainz_API/Search)

#### `POST /api/admin/metadata/enrich-all`

Run a bounded, paginated pass over eligible local tracks missing a recording ID. Tracks imported after the run starts are eligible on the next run. Progress distinguishes enrichment, ambiguous results, misses and errors. The server spaces requests to the same MusicBrainz provider. [MusicBrainz rate limiting](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting)

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

Check local catalog entries for missing files, read errors and changes to a recorded SHA-256 digest. The response starts a background task.

#### `POST /api/admin/storage/sync`

Start the same reconciliation that runs automatically every 24 hours. It indexes existing files, imports new audio, recognizes byte-identical copies and updates paths for moved tracks. After a successful scan, it removes catalog entries for files that are still missing. Removing a track also removes its associated playlist, favorite and listening-history entries. Remote P2P tracks are excluded.

If a music source contains no audio, its missing tracks are preserved and the source is listed in `empty_sources`. The primary volume (`/`) and the optional `network-nfs` and `network-smb` mounts are checked independently. After verifying the mount, pass its name in `prune_empty` to authorize cleanup for that source, for example `POST /api/admin/storage/sync?prune_empty=network-nfs`. Multiple confirmed sources may be comma-separated.

#### `GET /api/admin/storage/task-status`

Poll the task status after starting a sync or integrity check. A completed sync reports `scanned`, `imported`, `moved`, `removed`, `skipped`, `errors` and `empty_sources`. Starting another task while one is running returns HTTP 409.

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
