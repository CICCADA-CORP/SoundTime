// TypeScript interfaces for SoundTime

export interface Track {
  id: string;
  title: string;
  artist_id: string;
  album_id: string | null;
  track_number: number | null;
  disc_number: number | null;
  duration_secs: number;
  genre: string | null;
  year: number | null;
  file_path: string;
  file_size: number;
  format: string;
  bitrate: number | null;
  sample_rate: number | null;
  musicbrainz_id: string | null;
  waveform_data: number[] | null;
  uploaded_by: string | null;
  play_count: number;
  created_at: string;
  // Best bitrate from federated sources
  best_bitrate?: number | null;
  best_source?: string | null;
  // Joined fields
  artist_name?: string;
  album_title?: string;
  cover_url?: string;
}

export interface Album {
  id: string;
  title: string;
  artist_id: string;
  release_date: string | null;
  cover_url: string | null;
  genre: string | null;
  year: number | null;
  created_at: string;
  // Joined
  artist_name?: string;
  tracks?: Track[];
}

export interface Artist {
  id: string;
  name: string;
  bio: string | null;
  image_url: string | null;
  created_at: string;
  // Joined
  albums?: Album[];
  tracks?: Track[];
}

export interface Playlist {
  id: string;
  name: string;
  description: string | null;
  is_public: boolean;
  is_editorial?: boolean;
  owner_id: string;
  user_id?: string;
  cover_url: string | null;
  created_at: string;
  updated_at: string;
  // Joined
  owner_username?: string;
  tracks?: Track[];
  track_count?: number;
}

export interface User {
  id: string;
  username: string;
  email: string;
  display_name: string | null;
  avatar_url: string | null;
  role: string;
  instance_id: string;
  is_banned?: boolean;
  ban_reason?: string | null;
  banned_at?: string | null;
  created_at: string;
}

export interface ListenHistory {
  id: string;
  track_id: string;
  listened_at: string;
  track?: Track;
}

export interface Favorite {
  user_id: string;
  track_id: string;
  created_at: string;
  track?: Track;
}

export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  per_page: number;
  total_pages: number;
}

export interface SearchResults {
  tracks: Track[];
  albums: Album[];
  artists: Artist[];
  total: number;
  p2p_results?: NetworkSearchResult[];
}

/** A search result from a remote P2P peer. */
export interface NetworkSearchResult {
  hash: string;
  title: string;
  artist_name: string;
  album_title?: string;
  duration_secs: number;
  format: string;
  genre?: string;
  year?: number;
  bitrate?: number;
  source_node: string;
  musicbrainz_id?: string;
  relevance: number;
}

export interface NetworkSearchResponse {
  results: NetworkSearchResult[];
  total: number;
}

export interface TokenPair {
  access_token: string;
  refresh_token: string;
  token_type: string;
  expires_in: number;
}

export interface AuthResponse {
  user: User;
  tokens: TokenPair;
}

/** @deprecated Use AuthResponse instead */
export type LoginResponse = AuthResponse;
export type RegisterResponse = AuthResponse;

export interface UploadResponse {
  id: string;
  title: string;
  duration: number;
  format: string;
  message: string;
}

export interface ApiError {
  error: string;
}

// ─── Admin / P2P Types ──────────────────────────────────────────────

export interface AdminStats {
  total_users: number;
  total_tracks: number;
  total_blocked_domains: number;
  total_remote_tracks: number;
  p2p_enabled: boolean;
  p2p_node_id: string | null;
}

export interface InstanceSetting {
  key: string;
  value: string;
}

export interface BlockedDomain {
  id: string;
  domain: string;
  reason: string | null;
  created_at: string;
}

export interface KnownInstance {
  domain: string;
  track_count: number;
  is_blocked: boolean;
}

export interface P2pStatus {
  enabled: boolean;
  node_id: string | null;
  relay_url: string | null;
  relay_connected: boolean;
  direct_addresses: number;
  peer_count: number;
  online_peer_count: number;
}

export interface P2pPeer {
  node_id: string;
  name: string | null;
  version: string | null;
  track_count: number;
  last_seen: string;
  is_online: boolean;
}

export interface NetworkGraphNode {
  id: string;
  node_type: "self" | "peer" | "relay";
  label: string;
  online: boolean;
  track_count?: number;
  version?: string;
}

export interface NetworkGraphLink {
  source: string;
  target: string;
  link_type: "relay" | "direct" | "peer";
}

export interface NetworkGraph {
  nodes: NetworkGraphNode[];
  links: NetworkGraphLink[];
}

// ─── P2P Logs ────────────────────────────────────────────────────

export interface P2pLogEntry {
  timestamp: string;
  level: string;
  target: string;
  message: string;
  fields?: string[];
}

export interface P2pLogResponse {
  entries: P2pLogEntry[];
  total_in_buffer: number;
}

export interface MetadataStatus {
  total_tracks: number;
  enriched_tracks: number;
  pending_tracks: number;
  tracks_with_bitrate: number;
  total_albums: number;
  albums_with_cover: number;
  total_remote_tracks: number;
  available_remote_tracks: number;
}

export interface MetadataResult {
  track_id: string;
  status: string;
  recording_mbid: string | null;
  corrected_title: string | null;
  artist_mbid: string | null;
  artist_name: string | null;
  album_mbid: string | null;
  album_title: string | null;
  genre: string | null;
  year: number | null;
  cover_url: string | null;
}

export interface RemoteTrack {
  id: string;
  local_track_id: string | null;
  title: string;
  artist_name: string;
  album_title: string | null;
  instance_domain: string;
  remote_uri: string;
  bitrate: number | null;
  sample_rate: number | null;
  format: string | null;
  is_available: boolean;
  last_checked_at: string | null;
  created_at: string;
}

export interface HealthCheckResult {
  checked: number;
  instances: { domain: string; is_available: boolean }[];
}

// ─── Track Credits ──────────────────────────────────────────────────

export interface TrackCredits {
  id: string;
  title: string;
  duration_secs: number;
  format: string;
  bitrate: number | null;
  sample_rate: number | null;
  genre: string | null;
  year: number | null;
  track_number: number | null;
  disc_number: number | null;
  musicbrainz_id: string | null;
  play_count: number;
  uploaded_by: string | null;
  uploaded_by_username?: string | null;
  created_at: string;
  artist_id: string;
  artist_name: string;
  artist?: string;
  artist_bio: string | null;
  artist_image: string | null;
  artist_musicbrainz_id: string | null;
  album_id: string | null;
  album_title: string | null;
  album?: string | null;
  album_cover_url: string | null;
  album_genre: string | null;
  album_year: number | null;
  album_musicbrainz_id: string | null;
  best_bitrate?: number | null;
  best_source?: string | null;
}

// ─── Setup / Onboarding ─────────────────────────────────────────────

export interface SetupStatus {
  setup_complete: boolean;
  has_admin: boolean;
  instance_private: boolean;
}

export interface SetupAdminRequest {
  username: string;
  email: string;
  password: string;
}

export interface SetupInstanceRequest {
  instance_name: string;
  instance_description: string;
}

export interface SetupCompleteRequest {
  open_registrations: boolean;
  max_upload_size_mb: number;
  p2p_enabled: boolean;
}

// ─── Editorial / AI Playlists ───────────────────────────────────────

export interface EditorialPlaylist {
  id: string;
  name: string;
  description: string | null;
  cover_url: string | null;
  track_count: number;
  tracks: Track[];
}

export interface EditorialStatus {
  ai_configured: boolean;
  ai_base_url: string;
  ai_model: string;
  last_generated: string | null;
  playlist_count: number;
  needs_regeneration: boolean;
}

export interface EditorialGenerateResult {
  playlists_created: number;
  message: string;
}

// ─── Track Reports ──────────────────────────────────────────────────

export interface TrackReport {
  id: string;
  track_id: string;
  track_title: string;
  track_artist: string;
  is_local: boolean;
  reporter_username: string;
  reason: string;
  status: string;
  admin_note: string | null;
  created_at: string;
  resolved_at: string | null;
}

export interface ReportStats {
  pending: number;
  resolved: number;
  dismissed: number;
  total: number;
}

export interface AdminTrack {
  id: string;
  title: string;
  artist_name: string;
  is_local: boolean;
  format: string;
  play_count: number;
  report_count: number;
  created_at: string;
}

// ─── Terms of Service ───────────────────────────────────────────────

export interface TosResponse {
  content: string;
  is_default: boolean;
}

// ─── Storage Types ──────────────────────────────────────────────────

export interface StorageStatus {
  backend: string;
  total_tracks: number;
  total_size_bytes: number;
  storage_path_or_bucket: string;
  remote_track_count: number;
  remote_available_count: number;
}

export interface MissingTrack {
  track_id: string;
  title: string;
  file_path: string;
}

export interface IntegrityReport {
  total_checked: number;
  healthy: number;
  missing: MissingTrack[];
  errors: string[];
}

export interface SyncReport {
  scanned: number;
  imported: number;
  skipped: number;
  errors: string[];
}

export interface TaskProgress {
  processed: number;
  total: number | null;
}

export type StorageTaskStatus =
  | { status: "idle" }
  | { status: "running"; progress: TaskProgress }
  | { status: "completed"; result: { kind: "sync" } & SyncReport | { kind: "integrity" } & IntegrityReport }
  | { status: "error"; message: string };

// ─── Batch Upload ───────────────────────────────────────────────────

export interface BatchUploadItem {
  filename: string;
  success: boolean;
  track?: UploadResponse;
  error?: string;
}

export interface BatchUploadResponse {
  results: BatchUploadItem[];
  total: number;
  success: number;
  failed: number;
}

export interface ListingStatus {
  enabled: boolean;
  domain: string;
  domain_is_local: boolean;
  listing_url: string;
  has_token: boolean;
  status: string;
  error: string | null;
  last_heartbeat: string | null;
}

// ─── P2P Library Sync ───────────────────────────────────────────────

export type SyncState = "synced" | "partial" | "not_synced" | "offline" | "empty";

export interface PeerSyncStatus {
  node_id: string;
  name: string | null;
  version: string | null;
  is_online: boolean;
  peer_announced_tracks: number;
  local_remote_tracks: number;
  available_tracks: number;
  our_track_count: number;
  sync_ratio: number;
  sync_state: SyncState;
  last_seen: string;
}

export interface LibrarySyncOverview {
  local_track_count: number;
  total_peers: number;
  synced_peers: number;
  partial_peers: number;
  not_synced_peers: number;
  peers: PeerSyncStatus[];
}

export interface SyncProgress {
  processed: number;
  total: number | null;
  phase: string;
}

export interface SyncResult {
  peer_id: string;
  tracks_synced: number;
  tracks_already_known: number;
  errors: number;
  duration_secs: number;
}

export type LibrarySyncTaskStatus =
  | { status: "idle" }
  | { status: "running"; peer_id: string; progress: SyncProgress }
  | { status: "completed"; result: SyncResult }
  | { status: "error"; message: string };

// ─── Plugins ────────────────────────────────────────────────────────

export interface Plugin {
  id: string;
  name: string;
  version: string;
  description: string | null;
  author: string | null;
  license: string | null;
  homepage: string | null;
  git_url: string;
  permissions: PluginPermissions;
  status: "disabled" | "enabled" | "error";
  error_message: string | null;
  installed_at: string;
  updated_at: string;
}

export interface PluginPermissions {
  http_hosts: string[];
  events: string[];
  write_tracks: boolean;
  config_access: boolean;
  read_users: boolean;
}

export interface PluginConfig {
  key: string;
  value: string;
}

export interface PluginEventLog {
  id: string;
  plugin_id: string;
  event_name: string;
  payload: unknown;
  result: "success" | "error" | "timeout";
  execution_time_ms: number;
  error_message: string | null;
  created_at: string;
}

export interface PluginListResponse {
  plugins: Plugin[];
}

export interface PluginConfigResponse {
  config: PluginConfig[];
}

export interface PluginLogsResponse {
  logs: PluginEventLog[];
  total: number;
  page: number;
  per_page: number;
}

// ─── Themes ─────────────────────────────────────────────────────────

export interface Theme {
  id: string;
  name: string;
  version: string;
  description: string | null;
  author: string | null;
  license: string | null;
  homepage: string | null;
  git_url: string;
  css_path: string;
  assets_path: string | null;
  status: "enabled" | "disabled";
  installed_at: string;
  updated_at: string;
  installed_by: string | null;
}

export interface ThemeListResponse {
  themes: Theme[];
}

// ─── Stats Overview ─────────────────────────────────────────────────

export interface StatsOverview {
  total_tracks: number;
  total_albums: number;
  total_artists: number;
  total_genres: number;
  total_duration_secs: number;
  peer_count: number;
}

export interface HistoryEntry {
  id: string;
  track_id: string;
  listened_at: string;
  track: Track;
}

// ─── Last.fm Scrobbling ─────────────────────────────────────────────

export interface LastfmStatus {
  connected: boolean;
  username: string | null;
  scrobble_enabled: boolean;
}

export interface LastfmConnectResponse {
  auth_url: string;
}

// ─── Radio ───────────────────────────────────────────────────────────

export type RadioSeedType = "track" | "artist" | "genre" | "personal_mix";

export interface RadioNextRequest {
  seed_type: RadioSeedType;
  seed_id?: string;
  genre?: string;
  count?: number;
  exclude: string[];
}

export interface RadioNextResponse {
  tracks: Track[];
  exhausted: boolean;
}
