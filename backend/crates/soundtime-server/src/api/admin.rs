//! Admin API — P2P settings, instance management, blocked domains, monitoring, metadata

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::metadata_lookup;
use soundtime_db::entities::{blocked_domain, instance_setting, remote_track, track, user};

/// Extract p2p node from type-erased state
fn get_p2p_node(state: &soundtime_db::AppState) -> Option<Arc<soundtime_p2p::P2pNode>> {
    state
        .p2p
        .as_ref()
        .and_then(|any| any.clone().downcast::<soundtime_p2p::P2pNode>().ok())
}
use soundtime_db::AppState;

// ─── Settings ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SettingResponse {
    pub key: String,
    pub value: String,
}

/// GET /api/admin/settings — list all instance settings
pub async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SettingResponse>>, StatusCode> {
    let settings = instance_setting::Entity::find()
        .order_by_asc(instance_setting::Column::Key)
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        settings
            .into_iter()
            .map(|s| SettingResponse {
                key: s.key,
                value: s.value,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct UpdateSettingRequest {
    pub value: String,
}

/// PUT /api/admin/settings/:key — update a single setting
pub async fn update_setting(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<UpdateSettingRequest>,
) -> Result<Json<SettingResponse>, (StatusCode, Json<serde_json::Value>)> {
    let existing = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(&key))
        .one(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "DB error" })),
            )
        })?;

    match existing {
        Some(s) => {
            let mut update: instance_setting::ActiveModel = s.into();
            update.value = Set(body.value.clone());
            update.updated_at = Set(chrono::Utc::now().into());
            update.update(&state.db).await.map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Update failed" })),
                )
            })?;
        }
        None => {
            // Create new setting
            instance_setting::ActiveModel {
                id: Set(Uuid::new_v4()),
                key: Set(key.clone()),
                value: Set(body.value.clone()),
                updated_at: Set(chrono::Utc::now().into()),
            }
            .insert(&state.db)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Insert failed" })),
                )
            })?;
        }
    }

    Ok(Json(SettingResponse {
        key,
        value: body.value,
    }))
}

// ─── Blocked Domains ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BlockedDomainResponse {
    pub id: Uuid,
    pub domain: String,
    pub reason: Option<String>,
    pub created_at: String,
}

/// GET /api/admin/blocked-domains
pub async fn list_blocked_domains(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BlockedDomainResponse>>, StatusCode> {
    let domains = blocked_domain::Entity::find()
        .order_by_desc(blocked_domain::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        domains
            .into_iter()
            .map(|d| BlockedDomainResponse {
                id: d.id,
                domain: d.domain,
                reason: d.reason,
                created_at: d.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct BlockDomainRequest {
    pub domain: String,
    pub reason: Option<String>,
}

/// POST /api/admin/blocked-domains
pub async fn block_domain(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<BlockDomainRequest>,
) -> Result<(StatusCode, Json<BlockedDomainResponse>), (StatusCode, Json<serde_json::Value>)> {
    // Check if already blocked
    let existing = blocked_domain::Entity::find()
        .filter(blocked_domain::Column::Domain.eq(&body.domain))
        .one(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "DB error" })),
            )
        })?;

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Domain already blocked" })),
        ));
    }

    let now = chrono::Utc::now();
    let id = Uuid::new_v4();

    blocked_domain::ActiveModel {
        id: Set(id),
        domain: Set(body.domain.clone()),
        reason: Set(body.reason.clone()),
        blocked_by: Set(Some(user.0.sub)),
        created_at: Set(now.into()),
    }
    .insert(&state.db)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Insert failed" })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(BlockedDomainResponse {
            id,
            domain: body.domain,
            reason: body.reason,
            created_at: now.to_rfc3339(),
        }),
    ))
}

/// DELETE /api/admin/blocked-domains/:id
pub async fn unblock_domain(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = blocked_domain::Entity::delete_by_id(id)
        .exec(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Delete failed" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Domain not found" })),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ─── Blocklist Import / Export ──────────────────────────────────────

#[derive(Deserialize)]
pub struct ImportDomainEntry {
    pub domain: String,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
}

/// GET /api/admin/blocked-domains/export
pub async fn export_blocked_domains(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BlockedDomainResponse>>, StatusCode> {
    list_blocked_domains(State(state)).await
}

/// POST /api/admin/blocked-domains/import
pub async fn import_blocked_domains(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<Vec<ImportDomainEntry>>,
) -> Result<Json<ImportResult>, (StatusCode, Json<serde_json::Value>)> {
    let existing: std::collections::HashSet<String> = blocked_domain::Entity::find()
        .all(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "DB error"})),
            )
        })?
        .into_iter()
        .map(|d| d.domain)
        .collect();

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for entry in body {
        let domain = entry.domain.trim().to_lowercase();
        if domain.is_empty() || existing.contains(&domain) {
            skipped += 1;
            continue;
        }

        let now = chrono::Utc::now();
        blocked_domain::ActiveModel {
            id: Set(Uuid::new_v4()),
            domain: Set(domain),
            reason: Set(entry.reason),
            blocked_by: Set(Some(user.0.sub)),
            created_at: Set(now.into()),
        }
        .insert(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Insert failed"})),
            )
        })?;

        imported += 1;
    }

    Ok(Json(ImportResult { imported, skipped }))
}

// ─── Statistics ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AdminStats {
    pub total_users: u64,
    pub total_tracks: u64,
    pub total_blocked_domains: u64,
    pub total_remote_tracks: u64,
    pub p2p_enabled: bool,
    pub p2p_node_id: Option<String>,
}

/// GET /api/admin/stats
pub async fn get_stats(State(state): State<Arc<AppState>>) -> Result<Json<AdminStats>, StatusCode> {
    use soundtime_db::entities::track;

    let total_users = user::Entity::find().count(&state.db).await.unwrap_or(0);

    let total_tracks = track::Entity::find().count(&state.db).await.unwrap_or(0);

    let total_blocked_domains = blocked_domain::Entity::find()
        .count(&state.db)
        .await
        .unwrap_or(0);

    let total_remote_tracks = remote_track::Entity::find()
        .count(&state.db)
        .await
        .unwrap_or(0);

    let p2p_node = get_p2p_node(&state);
    let p2p_enabled = p2p_node.is_some();
    let p2p_node_id = p2p_node.map(|n| n.node_id().to_string());

    Ok(Json(AdminStats {
        total_users,
        total_tracks,
        total_blocked_domains,
        total_remote_tracks,
        p2p_enabled,
        p2p_node_id,
    }))
}

// ─── Public Node Info (for listing server) ──────────────────────────

#[derive(Serialize)]
pub struct NodeInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub track_count: u64,
    pub user_count: u64,
    pub open_registration: bool,
    pub p2p_enabled: bool,
    pub p2p_node_id: Option<String>,
}

/// GET /api/nodeinfo — public instance info consumed by the listing server
pub async fn nodeinfo(State(state): State<Arc<AppState>>) -> Result<Json<NodeInfo>, StatusCode> {
    let total_tracks = track::Entity::find().count(&state.db).await.unwrap_or(0);

    let total_users = user::Entity::find().count(&state.db).await.unwrap_or(0);

    let p2p_node = get_p2p_node(&state);
    let p2p_enabled = p2p_node.is_some();
    let p2p_node_id = p2p_node.map(|n| n.node_id().to_string());

    // Read instance settings
    let settings = instance_setting::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();

    let name = settings
        .iter()
        .find(|s| s.key == "instance_name")
        .map(|s| s.value.clone())
        .unwrap_or_else(|| "SoundTime".to_string());

    let description = settings
        .iter()
        .find(|s| s.key == "instance_description")
        .map(|s| s.value.clone())
        .unwrap_or_default();

    let open_registration = settings
        .iter()
        .find(|s| s.key == "instance_private")
        .map(|s| s.value != "true")
        .unwrap_or(true);

    Ok(Json(NodeInfo {
        name,
        description,
        version: "0.1.0".to_string(),
        track_count: total_tracks,
        user_count: total_users,
        open_registration,
        p2p_enabled,
        p2p_node_id,
    }))
}

// ─── Known Instances (from remote tracks) ───────────────────────────

#[derive(Serialize)]
pub struct KnownInstance {
    pub domain: String,
    pub track_count: usize,
    pub is_blocked: bool,
}

/// GET /api/admin/instances — known remote instances (derived from remote_tracks)
pub async fn list_instances(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<KnownInstance>>, StatusCode> {
    let remote_tracks = remote_track::Entity::find()
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let blocked = blocked_domain::Entity::find()
        .all(&state.db)
        .await
        .unwrap_or_default();

    let blocked_set: std::collections::HashSet<String> =
        blocked.into_iter().map(|b| b.domain).collect();

    let mut domain_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for rt in &remote_tracks {
        *domain_map.entry(rt.instance_domain.clone()).or_insert(0) += 1;
    }

    let mut instances: Vec<KnownInstance> = domain_map
        .into_iter()
        .map(|(domain, track_count)| KnownInstance {
            is_blocked: blocked_set.contains(&domain),
            domain,
            track_count,
        })
        .collect();

    instances.sort_by(|a, b| b.track_count.cmp(&a.track_count));

    Ok(Json(instances))
}

// ─── Users Management ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub role: String,
    pub is_banned: bool,
    pub ban_reason: Option<String>,
    pub banned_at: Option<String>,
    pub created_at: String,
}

/// GET /api/admin/users
pub async fn list_users(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<UserResponse>>, StatusCode> {
    let users = user::Entity::find()
        .order_by_asc(user::Column::Username)
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        users
            .into_iter()
            .map(|u| UserResponse {
                id: u.id,
                username: u.username,
                email: u.email,
                display_name: u.display_name,
                role: u.role.as_str().to_string(),
                is_banned: u.is_banned,
                ban_reason: u.ban_reason,
                banned_at: u.banned_at.map(|t| t.to_rfc3339()),
                created_at: u.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

/// PUT /api/admin/users/:id/role
pub async fn update_user_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserRoleRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    use soundtime_db::entities::user::UserRole;

    let role = match body.role.as_str() {
        "admin" => UserRole::Admin,
        "user" => UserRole::User,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid role. Use 'admin' or 'user'" })),
            ))
        }
    };

    let existing = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "DB error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "User not found" })),
            )
        })?;

    let mut update: user::ActiveModel = existing.into();
    update.role = Set(role);
    update.update(&state.db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Update failed" })),
        )
    })?;

    Ok(StatusCode::OK)
}

// ─── Ban / Unban Users ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BanUserRequest {
    pub reason: Option<String>,
}

/// PUT /api/admin/users/:id/ban
pub async fn ban_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<BanUserRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let existing = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "DB error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "User not found" })),
            )
        })?;

    // Prevent banning admins
    if existing.role == soundtime_db::entities::user::UserRole::Admin {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Cannot ban an admin user" })),
        ));
    }

    let mut update: user::ActiveModel = existing.into();
    update.is_banned = Set(true);
    update.ban_reason = Set(body.reason);
    update.banned_at = Set(Some(chrono::Utc::now().into()));
    update.update(&state.db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Update failed" })),
        )
    })?;

    tracing::info!(%id, "User banned");
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/users/:id/ban
pub async fn unban_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let existing = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "DB error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "User not found" })),
            )
        })?;

    let mut update: user::ActiveModel = existing.into();
    update.is_banned = Set(false);
    update.ban_reason = Set(None);
    update.banned_at = Set(None);
    update.update(&state.db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Update failed" })),
        )
    })?;

    tracing::info!(%id, "User unbanned");
    Ok(StatusCode::NO_CONTENT)
}

// ─── Metadata Enrichment ────────────────────────────────────────────

/// POST /api/admin/metadata/enrich/:track_id — enrich a single track via MusicBrainz
pub async fn enrich_track_metadata(
    State(state): State<Arc<AppState>>,
    Path(track_id): Path<Uuid>,
) -> Result<Json<metadata_lookup::MetadataResult>, (StatusCode, Json<serde_json::Value>)> {
    let result = metadata_lookup::enrich_track(&state.db, track_id).await;
    Ok(Json(result))
}

/// POST /api/admin/metadata/enrich-all — batch enrich all tracks without MusicBrainz IDs
pub async fn enrich_all_metadata(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<metadata_lookup::MetadataResult>>, StatusCode> {
    let results = metadata_lookup::enrich_all_tracks(&state.db).await;
    Ok(Json(results))
}

/// GET /api/admin/metadata/status — metadata enrichment status overview
pub async fn metadata_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MetadataStatusResponse>, StatusCode> {
    use soundtime_db::entities::track;

    let total_tracks = track::Entity::find().count(&state.db).await.unwrap_or(0);

    let enriched_tracks = track::Entity::find()
        .filter(track::Column::MusicbrainzId.is_not_null())
        .count(&state.db)
        .await
        .unwrap_or(0);

    let tracks_with_bitrate = track::Entity::find()
        .filter(track::Column::Bitrate.is_not_null())
        .count(&state.db)
        .await
        .unwrap_or(0);

    use soundtime_db::entities::album;
    let albums_with_cover = album::Entity::find()
        .filter(album::Column::CoverUrl.is_not_null())
        .count(&state.db)
        .await
        .unwrap_or(0);

    let total_albums = album::Entity::find().count(&state.db).await.unwrap_or(0);

    let total_remote_tracks = remote_track::Entity::find()
        .count(&state.db)
        .await
        .unwrap_or(0);

    let available_remote_tracks = remote_track::Entity::find()
        .filter(remote_track::Column::IsAvailable.eq(true))
        .count(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(MetadataStatusResponse {
        total_tracks,
        enriched_tracks,
        pending_tracks: total_tracks - enriched_tracks,
        tracks_with_bitrate,
        total_albums,
        albums_with_cover,
        total_remote_tracks,
        available_remote_tracks,
    }))
}

#[derive(Serialize)]
pub struct MetadataStatusResponse {
    pub total_tracks: u64,
    pub enriched_tracks: u64,
    pub pending_tracks: u64,
    pub tracks_with_bitrate: u64,
    pub total_albums: u64,
    pub albums_with_cover: u64,
    pub total_remote_tracks: u64,
    pub available_remote_tracks: u64,
}

// ─── Remote Tracks (Federated) ──────────────────────────────────────

#[derive(Serialize)]
pub struct RemoteTrackResponse {
    pub id: Uuid,
    pub local_track_id: Option<Uuid>,
    pub title: String,
    pub artist_name: String,
    pub album_title: Option<String>,
    pub instance_domain: String,
    pub remote_uri: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub format: Option<String>,
    pub is_available: bool,
    pub last_checked_at: Option<String>,
    pub created_at: String,
}

/// GET /api/admin/remote-tracks — list all remote tracks from federation
pub async fn list_remote_tracks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RemoteTrackResponse>>, StatusCode> {
    let tracks = remote_track::Entity::find()
        .order_by_desc(remote_track::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        tracks
            .into_iter()
            .take(200)
            .map(|t| RemoteTrackResponse {
                id: t.id,
                local_track_id: t.local_track_id,
                title: t.title,
                artist_name: t.artist_name,
                album_title: t.album_title,
                instance_domain: t.instance_domain,
                remote_uri: t.remote_uri,
                bitrate: t.bitrate,
                sample_rate: t.sample_rate,
                format: t.format,
                is_available: t.is_available,
                last_checked_at: t.last_checked_at.map(|d| d.to_rfc3339()),
                created_at: t.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

// ─── Instance Health Check ──────────────────────────────────────────

/// POST /api/admin/instances/health-check — check all known instances availability
pub async fn check_instances_health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let remote_tracks = remote_track::Entity::find()
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut domains: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rt in &remote_tracks {
        // Skip P2P tracks — they use iroh, not HTTP NodeInfo
        if rt.instance_domain.starts_with("p2p://") {
            continue;
        }
        domains.insert(rt.instance_domain.clone());
    }

    let mut results: Vec<serde_json::Value> = Vec::new();
    for domain in &domains {
        let is_available = metadata_lookup::check_instance_health(domain).await;
        results.push(serde_json::json!({
            "domain": domain,
            "is_available": is_available,
        }));
    }

    metadata_lookup::refresh_instance_availability(&state.db).await;

    Ok(Json(serde_json::json!({
        "checked": results.len(),
        "instances": results,
    })))
}

// ─── Storage Management ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct StorageStatusResponse {
    pub backend: String,
    pub total_tracks: u64,
    pub total_size_bytes: i64,
    pub storage_path_or_bucket: String,
    pub remote_track_count: u64,
    pub remote_available_count: u64,
}

/// GET /api/admin/storage/status
pub async fn storage_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StorageStatusResponse>, StatusCode> {
    let tracks = track::Entity::find()
        .all(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_size: i64 = tracks.iter().map(|t| t.file_size).sum();

    let remote_track_count = remote_track::Entity::find()
        .count(&state.db)
        .await
        .unwrap_or(0);
    let remote_available_count = remote_track::Entity::find()
        .filter(remote_track::Column::IsAvailable.eq(true))
        .count(&state.db)
        .await
        .unwrap_or(0);

    let backend_type = std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string());
    let storage_info = if backend_type == "s3" {
        std::env::var("S3_BUCKET").unwrap_or_else(|_| "N/A".to_string())
    } else {
        std::env::var("AUDIO_STORAGE_PATH").unwrap_or_else(|_| "./data/music".to_string())
    };

    Ok(Json(StorageStatusResponse {
        backend: backend_type,
        total_tracks: tracks.len() as u64,
        total_size_bytes: total_size,
        storage_path_or_bucket: storage_info,
        remote_track_count,
        remote_available_count,
    }))
}

/// POST /api/admin/storage/integrity-check
///
/// Launches the integrity check as a background task and returns immediately.
pub async fn run_integrity_check(
    State(state): State<Arc<AppState>>,
    Extension(tracker): Extension<crate::storage_worker::TaskTrackerHandle>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Reject if a task is already running
    {
        let lock = tracker.lock().await;
        if let Some(crate::storage_worker::TaskStatus::Running { .. }) = &*lock {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "A storage task is already running" })),
            ));
        }
    }

    // Mark as running
    {
        let mut lock = tracker.lock().await;
        *lock = Some(crate::storage_worker::TaskStatus::Running {
            progress: crate::storage_worker::TaskProgress {
                processed: 0,
                total: None,
            },
        });
    }

    let tracker_clone = tracker.clone();
    tokio::spawn(async move {
        match crate::storage_worker::run_integrity_check(&state, Some(&tracker_clone)).await {
            Ok(report) => {
                let mut lock = tracker_clone.lock().await;
                *lock = Some(crate::storage_worker::TaskStatus::Completed {
                    result: crate::storage_worker::TaskResult::Integrity(report),
                });
            }
            Err(e) => {
                let mut lock = tracker_clone.lock().await;
                *lock = Some(crate::storage_worker::TaskStatus::Error { message: e });
            }
        }
    });

    Ok(Json(
        serde_json::json!({ "status": "started", "task": "integrity-check" }),
    ))
}

/// POST /api/admin/storage/sync
///
/// Launches the storage sync as a background task and returns immediately.
pub async fn run_storage_sync(
    State(state): State<Arc<AppState>>,
    Extension(tracker): Extension<crate::storage_worker::TaskTrackerHandle>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Reject if a task is already running
    {
        let lock = tracker.lock().await;
        if let Some(crate::storage_worker::TaskStatus::Running { .. }) = &*lock {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "A storage task is already running" })),
            ));
        }
    }

    // Mark as running
    {
        let mut lock = tracker.lock().await;
        *lock = Some(crate::storage_worker::TaskStatus::Running {
            progress: crate::storage_worker::TaskProgress {
                processed: 0,
                total: None,
            },
        });
    }

    let tracker_clone = tracker.clone();
    tokio::spawn(async move {
        match crate::storage_worker::run_sync(&state, Some(&tracker_clone)).await {
            Ok(report) => {
                let mut lock = tracker_clone.lock().await;
                *lock = Some(crate::storage_worker::TaskStatus::Completed {
                    result: crate::storage_worker::TaskResult::Sync(report),
                });
            }
            Err(e) => {
                let mut lock = tracker_clone.lock().await;
                *lock = Some(crate::storage_worker::TaskStatus::Error { message: e });
            }
        }
    });

    Ok(Json(
        serde_json::json!({ "status": "started", "task": "sync" }),
    ))
}

/// GET /api/admin/storage/task-status
///
/// Returns the current status of a running or completed storage task.
pub async fn storage_task_status(
    Extension(tracker): Extension<crate::storage_worker::TaskTrackerHandle>,
) -> Json<serde_json::Value> {
    let lock = tracker.lock().await;
    match &*lock {
        Some(status) => Json(serde_json::json!(status)),
        None => Json(serde_json::json!({ "status": "idle" })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1. SettingResponse serialization
    #[test]
    fn test_serialize_setting_response() {
        let resp = SettingResponse {
            key: "foo".to_string(),
            value: "bar".to_string(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["key"], "foo");
        assert_eq!(val["value"], "bar");
    }

    // 2. BlockedDomainResponse serialization
    #[test]
    fn test_serialize_blocked_domain_response() {
        let resp = BlockedDomainResponse {
            id: Uuid::new_v4(),
            domain: "evil.com".to_string(),
            reason: Some("spam".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["domain"], "evil.com");
        assert_eq!(val["reason"], "spam");
    }

    // 3. AdminStats serialization
    #[test]
    fn test_serialize_admin_stats() {
        let stats = AdminStats {
            total_users: 42,
            total_tracks: 100,
            total_blocked_domains: 3,
            total_remote_tracks: 50,
            p2p_enabled: true,
            p2p_node_id: Some("abc123".to_string()),
        };
        let val = serde_json::to_value(&stats).unwrap();
        assert_eq!(val["total_users"], 42);
        assert_eq!(val["p2p_enabled"], true);
        assert_eq!(val["p2p_node_id"], "abc123");
    }

    // 4. AdminStats without P2P
    #[test]
    fn test_serialize_admin_stats_no_p2p() {
        let stats = AdminStats {
            total_users: 1,
            total_tracks: 0,
            total_blocked_domains: 0,
            total_remote_tracks: 0,
            p2p_enabled: false,
            p2p_node_id: None,
        };
        let val = serde_json::to_value(&stats).unwrap();
        assert_eq!(val["p2p_enabled"], false);
        assert!(val["p2p_node_id"].is_null());
    }

    // 5. NodeInfo serialization
    #[test]
    fn test_serialize_node_info() {
        let info = NodeInfo {
            name: "My Instance".to_string(),
            description: "A test instance".to_string(),
            version: "0.1.0".to_string(),
            track_count: 100,
            user_count: 10,
            open_registration: true,
            p2p_enabled: false,
            p2p_node_id: None,
        };
        let val = serde_json::to_value(&info).unwrap();
        assert_eq!(val["name"], "My Instance");
        assert_eq!(val["version"], "0.1.0");
        assert_eq!(val["open_registration"], true);
    }

    // 6. ImportResult serialization
    #[test]
    fn test_serialize_import_result() {
        let result = ImportResult {
            imported: 5,
            skipped: 2,
        };
        let val = serde_json::to_value(&result).unwrap();
        assert_eq!(val["imported"], 5);
        assert_eq!(val["skipped"], 2);
    }

    // 7. KnownInstance serialization
    #[test]
    fn test_serialize_known_instance() {
        let inst = KnownInstance {
            domain: "other.example.com".to_string(),
            track_count: 42,
            is_blocked: false,
        };
        let val = serde_json::to_value(&inst).unwrap();
        assert_eq!(val["domain"], "other.example.com");
        assert_eq!(val["track_count"], 42);
        assert_eq!(val["is_blocked"], false);
    }

    // 8. UserResponse serialization
    #[test]
    fn test_serialize_user_response() {
        let resp = UserResponse {
            id: Uuid::new_v4(),
            username: "alice".to_string(),
            email: "alice@example.com".to_string(),
            display_name: Some("Alice".to_string()),
            role: "admin".to_string(),
            is_banned: false,
            ban_reason: None,
            banned_at: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["username"], "alice");
        assert_eq!(val["role"], "admin");
        assert!(!val["is_banned"].as_bool().unwrap());
    }

    // 9. UpdateUserRoleRequest deserialization
    #[test]
    fn test_deserialize_update_user_role_request() {
        let json = r#"{"role":"admin"}"#;
        let req: UpdateUserRoleRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.role, "admin");
    }

    // 10. BanUserRequest deserialization
    #[test]
    fn test_deserialize_ban_user_request() {
        let json = r#"{"reason":"spam"}"#;
        let req: BanUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.reason, Some("spam".to_string()));
    }

    // 11. BanUserRequest without reason
    #[test]
    fn test_deserialize_ban_user_request_no_reason() {
        let json = r#"{}"#;
        let req: BanUserRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.reason, None);
    }

    // 12. MetadataStatusResponse serialization
    #[test]
    fn test_serialize_metadata_status_response() {
        let resp = MetadataStatusResponse {
            total_tracks: 100,
            enriched_tracks: 80,
            pending_tracks: 20,
            tracks_with_bitrate: 95,
            total_albums: 10,
            albums_with_cover: 8,
            total_remote_tracks: 50,
            available_remote_tracks: 45,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["total_tracks"], 100);
        assert_eq!(val["pending_tracks"], 20);
    }

    // 13. StorageStatusResponse serialization
    #[test]
    fn test_serialize_storage_status_response() {
        let resp = StorageStatusResponse {
            backend: "local".to_string(),
            total_tracks: 50,
            total_size_bytes: 1_000_000,
            storage_path_or_bucket: "/data/music".to_string(),
            remote_track_count: 20,
            remote_available_count: 15,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["backend"], "local");
        assert_eq!(val["total_size_bytes"], 1_000_000);
    }

    // 14. RemoteTrackResponse serialization
    #[test]
    fn test_serialize_remote_track_response() {
        let resp = RemoteTrackResponse {
            id: Uuid::new_v4(),
            local_track_id: None,
            title: "Song".to_string(),
            artist_name: "Artist".to_string(),
            album_title: Some("Album".to_string()),
            instance_domain: "other.com".to_string(),
            remote_uri: "https://other.com/api/audio/stream/123".to_string(),
            bitrate: Some(320),
            sample_rate: Some(44100),
            format: Some("mp3".to_string()),
            is_available: true,
            last_checked_at: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["title"], "Song");
        assert_eq!(val["bitrate"], 320);
        assert!(val["local_track_id"].is_null());
    }

    // 15. update_user_role invalid role validation (runs BEFORE DB access)
    #[tokio::test]
    async fn test_update_user_role_invalid_role() {
        use axum::{body::Body, http::Request, routing::put, Router};
        use tower::ServiceExt;

        fn test_state() -> Arc<AppState> {
            Arc::new(AppState {
                db: sea_orm::DatabaseConnection::Disconnected,
                jwt_secret: "test-secret".to_string(),
                domain: "localhost".to_string(),
                storage: Arc::new(soundtime_audio::AudioStorage::new("/tmp/test")),
                p2p: None,
                plugins: None,
            })
        }

        let state = test_state();
        let app = Router::new()
            .route("/users/{id}/role", put(update_user_role))
            .with_state(state);

        let user_id = Uuid::new_v4();
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/users/{user_id}/role"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"role":"superadmin"}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // 16. UpdateSettingRequest deserialization
    #[test]
    fn test_deserialize_update_setting_request() {
        let json = r#"{"value":"new_value"}"#;
        let req: UpdateSettingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.value, "new_value");
    }

    // 17. BlockDomainRequest deserialization
    #[test]
    fn test_deserialize_block_domain_request() {
        let json = r#"{"domain":"bad.com","reason":"malware"}"#;
        let req: BlockDomainRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.domain, "bad.com");
        assert_eq!(req.reason, Some("malware".to_string()));
    }

    // 18. ImportDomainEntry deserialization
    #[test]
    fn test_deserialize_import_domain_entry() {
        let json = r#"{"domain":"spam.org","reason":null}"#;
        let entry: ImportDomainEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.domain, "spam.org");
        assert_eq!(entry.reason, None);
    }

    // 19. get_p2p_node returns None when state.p2p is None
    #[test]
    fn test_get_p2p_node_returns_none() {
        let state = AppState {
            db: sea_orm::DatabaseConnection::Disconnected,
            jwt_secret: "test".to_string(),
            domain: "localhost".to_string(),
            storage: Arc::new(soundtime_audio::AudioStorage::new("/tmp/test")),
            p2p: None,
            plugins: None,
        };
        assert!(get_p2p_node(&state).is_none());
    }
}
