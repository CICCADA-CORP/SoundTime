//! Track reports & Terms of Service API
//!
//! - Users can report tracks (POST /api/tracks/:id/report)
//! - Admins can list/resolve/dismiss reports (GET/PUT /api/admin/reports/...)
//! - Admins can manage ToS (stored as `tos_content` in instance_settings)
//! - Public ToS endpoint: GET /api/tos

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
use soundtime_db::entities::{
    artist, instance_setting, playlist_track, remote_track, track, track_report, user,
};
use soundtime_db::AppState;

// ═══════════════════════════════════════════════════════════════════
// USER: Report a track
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct ReportRequest {
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ReportResponse {
    pub id: Uuid,
    pub message: String,
}

/// POST /api/tracks/:id/report — report a track
pub async fn report_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(track_id): Path<Uuid>,
    Json(body): Json<ReportRequest>,
) -> Result<Json<ReportResponse>, (StatusCode, Json<serde_json::Value>)> {
    let reason = body.reason.trim().to_string();
    if reason.is_empty() || reason.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "La raison doit faire entre 1 et 500 caractères." })),
        ));
    }

    // Verify track exists
    track::Entity::find_by_id(track_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Piste introuvable." })),
            )
        })?;

    // Check for duplicate pending report from same user
    let existing = track_report::Entity::find()
        .filter(track_report::Column::TrackId.eq(track_id))
        .filter(track_report::Column::UserId.eq(user.0.sub))
        .filter(track_report::Column::Status.eq("pending"))
        .one(&state.db)
        .await
        .unwrap_or(None);

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Vous avez déjà signalé cette piste." })),
        ));
    }

    let report_id = Uuid::new_v4();
    let now = chrono::Utc::now().fixed_offset();

    let report = track_report::ActiveModel {
        id: Set(report_id),
        track_id: Set(track_id),
        user_id: Set(user.0.sub),
        reason: Set(reason),
        status: Set("pending".to_string()),
        admin_note: Set(None),
        resolved_by: Set(None),
        resolved_at: Set(None),
        created_at: Set(now),
    };

    report.insert(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    tracing::info!(track_id = %track_id, user_id = %user.0.sub, "Track reported");

    Ok(Json(ReportResponse {
        id: report_id,
        message: "Signalement enregistré. L'administrateur examinera votre demande.".to_string(),
    }))
}

// ═══════════════════════════════════════════════════════════════════
// ADMIN: List & manage reports
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct AdminReportResponse {
    pub id: Uuid,
    pub track_id: Uuid,
    pub track_title: String,
    pub track_artist: String,
    pub is_local: bool,
    pub reporter_username: String,
    pub reason: String,
    pub status: String,
    pub admin_note: Option<String>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    pub resolved_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

/// GET /api/admin/reports — list all reports (pending first)
pub async fn list_reports(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AdminReportResponse>>, (StatusCode, String)> {
    let reports = track_report::Entity::find()
        .order_by_asc(track_report::Column::Status) // "pending" before "resolved"
        .order_by_desc(track_report::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let mut results = Vec::new();
    for r in reports {
        // Get track info
        let t = track::Entity::find_by_id(r.track_id)
            .one(&state.db)
            .await
            .unwrap_or(None);

        let (track_title, track_artist, is_local) = match t {
            Some(ref trk) => {
                let artist_name = artist::Entity::find_by_id(trk.artist_id)
                    .one(&state.db)
                    .await
                    .ok()
                    .flatten()
                    .map(|a| a.name)
                    .unwrap_or_else(|| "Inconnu".to_string());
                let is_local = true;
                (trk.title.clone(), artist_name, is_local)
            }
            None => ("[Supprimé]".to_string(), "".to_string(), true),
        };

        let reporter = user::Entity::find_by_id(r.user_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_else(|| "inconnu".to_string());

        results.push(AdminReportResponse {
            id: r.id,
            track_id: r.track_id,
            track_title,
            track_artist,
            is_local,
            reporter_username: reporter,
            reason: r.reason,
            status: r.status,
            admin_note: r.admin_note,
            created_at: r.created_at,
            resolved_at: r.resolved_at,
        });
    }

    Ok(Json(results))
}

#[derive(Debug, Deserialize)]
pub struct ResolveReportRequest {
    /// "resolved" | "dismissed"
    pub action: String,
    /// "delete" | "unlist" | "none"
    pub track_action: Option<String>,
    pub admin_note: Option<String>,
}

/// PUT /api/admin/reports/:id — resolve/dismiss a report
pub async fn resolve_report(
    State(state): State<Arc<AppState>>,
    Extension(admin): Extension<AuthUser>,
    Path(report_id): Path<Uuid>,
    Json(body): Json<ResolveReportRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let report = track_report::Entity::find_by_id(report_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Signalement introuvable." })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();
    let status = match body.action.as_str() {
        "resolved" => "resolved",
        "dismissed" => "dismissed",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Action invalide (resolved ou dismissed)." })),
            ));
        }
    };

    // Apply track action if the report is being resolved
    let track_action = body.track_action.as_deref().unwrap_or("none");
    let mut track_action_msg = String::new();

    if status == "resolved" {
        let track_model = track::Entity::find_by_id(report.track_id)
            .one(&state.db)
            .await
            .unwrap_or(None);

        if let Some(trk) = track_model {
            let is_local = true;

            match track_action {
                "delete" if is_local => {
                    // Delete local track: remove from playlists first, then delete
                    playlist_track::Entity::delete_many()
                        .filter(playlist_track::Column::TrackId.eq(trk.id))
                        .exec(&state.db)
                        .await
                        .ok();
                    track::Entity::delete_by_id(trk.id)
                        .exec(&state.db)
                        .await
                        .ok();
                    // Delete file
                    tokio::fs::remove_file(&trk.file_path).await.ok();
                    track_action_msg = "Piste locale supprimée.".to_string();
                    tracing::info!(track_id = %trk.id, "Local track deleted via report");
                }
                "unlist" if !is_local => {
                    // Unlist remote track: remove from remote_tracks table
                    remote_track::Entity::delete_many()
                        .filter(remote_track::Column::LocalTrackId.eq(Some(trk.id)))
                        .exec(&state.db)
                        .await
                        .ok();
                    // Delete the track entry (unlist it from this instance)
                    playlist_track::Entity::delete_many()
                        .filter(playlist_track::Column::TrackId.eq(trk.id))
                        .exec(&state.db)
                        .await
                        .ok();
                    track::Entity::delete_by_id(trk.id)
                        .exec(&state.db)
                        .await
                        .ok();
                    track_action_msg = "Piste distante déréférencée.".to_string();
                    tracing::info!(track_id = %trk.id, "Remote track unlisted via report");
                }
                "delete" if !is_local => {
                    // Can't delete remote — unlist instead
                    remote_track::Entity::delete_many()
                        .filter(remote_track::Column::LocalTrackId.eq(Some(trk.id)))
                        .exec(&state.db)
                        .await
                        .ok();
                    playlist_track::Entity::delete_many()
                        .filter(playlist_track::Column::TrackId.eq(trk.id))
                        .exec(&state.db)
                        .await
                        .ok();
                    track::Entity::delete_by_id(trk.id)
                        .exec(&state.db)
                        .await
                        .ok();
                    track_action_msg =
                        "Piste distante déréférencée (suppression impossible).".to_string();
                }
                _ => {}
            }
        }
    }

    // Update report
    let mut active: track_report::ActiveModel = report.into();
    active.status = Set(status.to_string());
    active.admin_note = Set(body.admin_note);
    active.resolved_by = Set(Some(admin.0.sub));
    active.resolved_at = Set(Some(now));
    active.update(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "message": format!("Signalement {}. {}", status, track_action_msg),
    })))
}

/// GET /api/admin/reports/stats — report statistics
pub async fn report_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pending = track_report::Entity::find()
        .filter(track_report::Column::Status.eq("pending"))
        .count(&state.db)
        .await
        .unwrap_or(0);

    let resolved = track_report::Entity::find()
        .filter(track_report::Column::Status.eq("resolved"))
        .count(&state.db)
        .await
        .unwrap_or(0);

    let dismissed = track_report::Entity::find()
        .filter(track_report::Column::Status.eq("dismissed"))
        .count(&state.db)
        .await
        .unwrap_or(0);

    Ok(Json(serde_json::json!({
        "pending": pending,
        "resolved": resolved,
        "dismissed": dismissed,
        "total": pending + resolved + dismissed,
    })))
}

// ═══════════════════════════════════════════════════════════════════
// ADMIN: Browse all tracks for moderation
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct TrackBrowseParams {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminTrackResponse {
    pub id: Uuid,
    pub title: String,
    pub artist_name: String,
    pub is_local: bool,
    pub format: String,
    pub play_count: i64,
    pub report_count: u64,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

/// GET /api/admin/tracks/browse — paginated track list for moderation
pub async fn browse_tracks(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<TrackBrowseParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(100);

    let mut query = track::Entity::find().order_by_desc(track::Column::CreatedAt);

    if let Some(ref search) = params.search {
        let s = search.trim();
        if !s.is_empty() {
            query = query.filter(track::Column::Title.contains(s));
        }
    }

    let total = query
        .clone()
        .count(&state.db)
        .await
        .unwrap_or(0);

    let tracks = query
        .paginate(&state.db, per_page)
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let mut results = Vec::new();
    for t in tracks {
        let artist_name = artist::Entity::find_by_id(t.artist_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|a| a.name)
            .unwrap_or_else(|| "Inconnu".to_string());

        let report_count = track_report::Entity::find()
            .filter(track_report::Column::TrackId.eq(t.id))
            .filter(track_report::Column::Status.eq("pending"))
            .count(&state.db)
            .await
            .unwrap_or(0);

        results.push(AdminTrackResponse {
            id: t.id,
            title: t.title,
            artist_name,
            is_local: true,
            format: t.format,
            play_count: t.play_count,
            report_count,
            created_at: t.created_at,
        });
    }

    let total_pages = (total as f64 / per_page as f64).ceil() as u64;

    Ok(Json(serde_json::json!({
        "data": results,
        "total": total,
        "page": page,
        "per_page": per_page,
        "total_pages": total_pages,
    })))
}

/// DELETE /api/admin/tracks/:id/moderate — delete or unlist a track (admin)
pub async fn moderate_track(
    State(state): State<Arc<AppState>>,
    Path(track_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let trk = track::Entity::find_by_id(track_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Piste introuvable." })),
            )
        })?;

    let is_local = true;

    // Remove from playlists
    playlist_track::Entity::delete_many()
        .filter(playlist_track::Column::TrackId.eq(trk.id))
        .exec(&state.db)
        .await
        .ok();

    if !is_local {
        // Unlist remote
        remote_track::Entity::delete_many()
            .filter(remote_track::Column::LocalTrackId.eq(Some(trk.id)))
            .exec(&state.db)
            .await
            .ok();
    }

    // Delete track entry
    track::Entity::delete_by_id(trk.id)
        .exec(&state.db)
        .await
        .ok();

    // Delete file if local
    if is_local {
        tokio::fs::remove_file(&trk.file_path).await.ok();
    }

    // Auto-resolve any pending reports for this track
    let pending_reports = track_report::Entity::find()
        .filter(track_report::Column::TrackId.eq(track_id))
        .filter(track_report::Column::Status.eq("pending"))
        .all(&state.db)
        .await
        .unwrap_or_default();

    let now = chrono::Utc::now().fixed_offset();
    for r in pending_reports {
        let mut active: track_report::ActiveModel = r.into();
        active.status = Set("resolved".to_string());
        active.admin_note = Set(Some("Piste supprimée/déréférencée par l'administrateur.".to_string()));
        active.resolved_at = Set(Some(now));
        active.update(&state.db).await.ok();
    }

    let msg = if is_local {
        "Piste locale supprimée."
    } else {
        "Piste distante déréférencée."
    };

    tracing::info!(track_id = %track_id, is_local, "Track moderated by admin");

    Ok(Json(serde_json::json!({ "message": msg })))
}

// ═══════════════════════════════════════════════════════════════════
// Terms of Service
// ═══════════════════════════════════════════════════════════════════

const DEFAULT_TOS: &str = r#"# Conditions d'utilisation — SoundTime

Bienvenue sur cette instance SoundTime. En utilisant ce service, vous acceptez les conditions suivantes :

## 1. Utilisation du service
- Ce service est une plateforme de streaming musical fédérée.
- Vous êtes responsable du contenu que vous uploadez.
- Tout contenu enfreignant les droits d'auteur peut être supprimé sans préavis.

## 2. Contenu interdit
- Contenu violant les droits de propriété intellectuelle.
- Contenu illégal, haineux, ou diffamatoire.
- Spam ou contenu non musical.

## 3. Signalements
- Les utilisateurs peuvent signaler du contenu qu'ils estiment problématique.
- L'administrateur se réserve le droit de supprimer tout contenu signalé.

## 4. Responsabilité
- L'administrateur de cette instance n'est pas responsable du contenu uploadé par les utilisateurs.
- Nous faisons nos meilleurs efforts pour modérer le contenu de manière réactive.

## 5. Données personnelles
- Seules les données nécessaires au fonctionnement du service sont collectées.
- Aucune donnée n'est revendue à des tiers.

## 6. Modifications
- Ces conditions peuvent être modifiées à tout moment par l'administrateur de l'instance.

---
*Dernière mise à jour : généré automatiquement par SoundTime.*
"#;

#[derive(Debug, Serialize)]
pub struct TosResponse {
    pub content: String,
    pub is_default: bool,
}

/// GET /api/tos — public, get Terms of Service
pub async fn get_tos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TosResponse>, StatusCode> {
    let setting = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("tos_content"))
        .one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match setting {
        Some(s) if !s.value.trim().is_empty() => Ok(Json(TosResponse {
            content: s.value,
            is_default: false,
        })),
        _ => Ok(Json(TosResponse {
            content: DEFAULT_TOS.to_string(),
            is_default: true,
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateTosRequest {
    pub content: String,
}

/// PUT /api/admin/tos — update Terms of Service
pub async fn update_tos(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpdateTosRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let now = chrono::Utc::now().fixed_offset();

    let existing = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("tos_content"))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if let Some(s) = existing {
        let mut active: instance_setting::ActiveModel = s.into();
        active.value = Set(body.content);
        active.updated_at = Set(now);
        active.update(&state.db).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
        })?;
    } else {
        let new_setting = instance_setting::ActiveModel {
            id: Set(Uuid::new_v4()),
            key: Set("tos_content".to_string()),
            value: Set(body.content),
            updated_at: Set(now),
        };
        new_setting.insert(&state.db).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
        })?;
    }

    Ok(Json(serde_json::json!({ "message": "Conditions d'utilisation mises à jour." })))
}

/// DELETE /api/admin/tos — reset ToS to default
pub async fn reset_tos(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    instance_setting::Entity::delete_many()
        .filter(instance_setting::Column::Key.eq("tos_content"))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "message": "Conditions d'utilisation réinitialisées au modèle par défaut." })))
}
