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
  track_count: number;
  last_seen: string;
  is_online: boolean;
}

export interface NetworkGraphNode {
  id: string;
  node_type: "self" | "peer" | "relay";
  label: string;
  online: boolean;
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
  created_at: string;
  artist_id: string;
  artist_name: string;
  artist_bio: string | null;
  artist_image: string | null;
  artist_musicbrainz_id: string | null;
  album_id: string | null;
  album_title: string | null;
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
