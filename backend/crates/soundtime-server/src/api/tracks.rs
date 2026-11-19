use axum::{
    extract::{Path, Query, State},
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
use soundtime_db::entities::{album, artist, remote_track, track};
use soundtime_db::AppState;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

#[derive(Debug, Serialize)]
pub struct TrackResponse {
    pub availability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
    pub id: Uuid,
    pub title: String,
    pub artist_id: Uuid,
    pub album_id: Option<Uuid>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub duration_secs: f32,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub musicbrainz_id: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub play_count: i64,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    /// Joined artist name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_name: Option<String>,
    /// Joined album title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_title: Option<String>,
    /// Album cover URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    /// Highest bitrate available across local + federated sources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bitrate: Option<i32>,
    /// Source providing the best bitrate ("local" or instance domain)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_source: Option<String>,
}

impl From<track::Model> for TrackResponse {
    fn from(t: track::Model) -> Self {
        Self {
            availability: "unavailable_on_instance".into(),
            position: None,
            id: t.id,
            title: t.title,
            artist_id: t.artist_id,
            album_id: t.album_id,
            track_number: t.track_number,
            disc_number: t.disc_number,
            duration_secs: t.duration_secs,
            genre: t.genre,
            year: t.year,
            format: t.format,
            bitrate: t.bitrate,
            sample_rate: t.sample_rate,
            musicbrainz_id: t.musicbrainz_id,
            uploaded_by: t.uploaded_by,
            play_count: t.play_count,
            created_at: t.created_at,
            artist_name: None,
            album_title: None,
            cover_url: None,
            best_bitrate: None,
            best_source: None,
        }
    }
}

impl TrackResponse {
    /// For rows selected with the playable catalogue predicate in the same SQL query.
    pub(super) fn playable(track: track::Model) -> Self {
        Self {
            availability: "playable".into(),
            ..Self::from(track)
        }
    }
}

fn policy_flags(state: &AppState) -> [bool; 2] {
    let node = state
        .p2p
        .as_ref()
        .and_then(|node| node.downcast_ref::<soundtime_p2p::P2pNode>());
    [
        node.is_some_and(|node| node.explicit_catalog()),
        node.is_some(),
    ]
}

pub(super) fn playable_filter(state: &AppState) -> sea_orm::sea_query::SimpleExpr {
    sea_orm::sea_query::Expr::cust_with_values(
        "tracks.id IN (SELECT track_id FROM federation_track_availability($1,$2) WHERE availability='playable')",
        policy_flags(state),
    )
}

/// Personal collections retain their saved rows. Resolve policy once for the page,
/// including repeated appearances of a track in the listening history.
pub(super) async fn attach_availability<'a>(
    state: &AppState,
    tracks: impl IntoIterator<Item = &'a mut TrackResponse>,
) -> Result<(), (StatusCode, String)> {
    use sea_orm::{DbBackend, FromQueryResult, Statement};
    #[derive(FromQueryResult)]
    struct Decision {
        track_id: Uuid,
        availability: String,
    }
    let tracks: Vec<_> = tracks.into_iter().collect();
    if tracks.is_empty() {
        return Ok(());
    }
    let ids: std::collections::HashSet<_> = tracks.iter().map(|t| t.id).collect();
    let [strict, enabled] = policy_flags(state);
    let query = Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT track_id, availability FROM federation_track_availability($1,$2)
         WHERE track_id IN (SELECT value::uuid FROM jsonb_array_elements_text($3) AS requested(value))",
        [strict.into(), enabled.into(), serde_json::json!(ids).into()]);
    let rows = Decision::find_by_statement(query)
        .all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(%e, "track availability lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Track availability unavailable".to_owned(),
            )
        })?;
    let states: HashMap<_, _> = rows
        .into_iter()
        .map(|row| (row.track_id, row.availability))
        .collect();
    for track in tracks {
        if let Some(state) = states.get(&track.id) {
            track.availability.clone_from(state);
        }
    }
    Ok(())
}

/// GET /api/tracks
pub async fn list_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = track::Entity::find()
        .filter(playable_filter(&state))
        .order_by_desc(track::Column::CreatedAt)
        .order_by_asc(track::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/tracks/my-uploads – returns only the authenticated user's uploads
pub async fn my_uploads(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let user_id = user.0.sub;
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = track::Entity::find()
        .filter(track::Column::UploadedBy.eq(user_id))
        .order_by_desc(track::Column::CreatedAt)
        .order_by_asc(track::Column::Id)
        .order_by_asc(track::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let mut data: Vec<_> = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::from(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    attach_availability(&state, &mut data).await?;
    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/tracks/:id
pub async fn get_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackResponse>, (StatusCode, String)> {
    let track_model = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    let mut resp = TrackResponse::from(track_model.clone());

    // Fetch artist name
    if let Ok(Some(a)) = artist::Entity::find_by_id(track_model.artist_id)
        .one(&state.db)
        .await
    {
        resp.artist_name = Some(a.name);
    }

    // Fetch album title & cover
    if let Some(album_id) = track_model.album_id {
        if let Ok(Some(a)) = album::Entity::find_by_id(album_id).one(&state.db).await {
            resp.album_title = Some(a.title);
            resp.cover_url = a.cover_url.map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") {
                    url
                } else {
                    format!("/api/media/{url}")
                }
            });
        }
    }

    // Compute best available bitrate across local and remote sources
    let local_bitrate = track_model.bitrate.unwrap_or(0);
    let mut best_bitrate = local_bitrate;
    let mut best_source = "local".to_string();

    // Check remote tracks linked to this one
    let remotes = remote_track::Entity::find()
        .filter(remote_track::Column::LocalTrackId.eq(Some(id)))
        .filter(remote_track::Column::IsAvailable.eq(true))
        .all(&state.db)
        .await
        .unwrap_or_default();

    for rt in &remotes {
        if rt.bitrate.unwrap_or(0) > best_bitrate {
            best_bitrate = rt.bitrate.unwrap_or(0);
            best_source = rt.instance_domain.clone();
        }
    }

    // Also check by musicbrainz_id
    if let Some(ref mbid) = track_model.musicbrainz_id {
        let remotes_mb = remote_track::Entity::find()
            .filter(remote_track::Column::MusicbrainzId.eq(mbid.clone()))
            .filter(remote_track::Column::IsAvailable.eq(true))
            .all(&state.db)
            .await
            .unwrap_or_default();

        for rt in &remotes_mb {
            if rt.bitrate.unwrap_or(0) > best_bitrate {
                best_bitrate = rt.bitrate.unwrap_or(0);
                best_source = rt.instance_domain.clone();
            }
        }
    }

    if best_bitrate > 0 {
        resp.best_bitrate = Some(best_bitrate);
        resp.best_source = Some(best_source);
    }

    attach_availability(&state, [&mut resp]).await?;
    if resp.availability != "playable" {
        resp.best_bitrate = None;
        resp.best_source = None;
    }
    Ok(Json(resp))
}

// ─── Track Credits / Metadata (public) ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct TrackCreditsResponse {
    pub id: Uuid,
    pub title: String,
    pub duration_secs: f32,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub musicbrainz_id: Option<String>,
    pub play_count: i64,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    // Artist info
    pub artist_id: Uuid,
    pub artist_name: String,
    pub artist_bio: Option<String>,
    pub artist_image: Option<String>,
    pub artist_musicbrainz_id: Option<String>,
    // Album info
    pub album_id: Option<Uuid>,
    pub album_title: Option<String>,
    pub album_cover_url: Option<String>,
    pub album_genre: Option<String>,
    pub album_year: Option<i16>,
    pub album_musicbrainz_id: Option<String>,
    // Best source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bitrate: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_source: Option<String>,
}

/// GET /api/tracks/:id/credits — full metadata & credits (public)
pub async fn get_track_credits(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackCreditsResponse>, (StatusCode, String)> {
    let track_model = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    let artist_model = artist::Entity::find_by_id(track_model.artist_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Artist not found".to_string()))?;

    let album_model = if let Some(album_id) = track_model.album_id {
        album::Entity::find_by_id(album_id)
            .one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    } else {
        None
    };

    Ok(Json(TrackCreditsResponse {
        id: track_model.id,
        title: track_model.title,
        duration_secs: track_model.duration_secs,
        format: track_model.format,
        bitrate: track_model.bitrate,
        sample_rate: track_model.sample_rate,
        genre: track_model.genre,
        year: track_model.year,
        track_number: track_model.track_number,
        disc_number: track_model.disc_number,
        musicbrainz_id: track_model.musicbrainz_id,
        play_count: track_model.play_count,
        uploaded_by: track_model.uploaded_by,
        created_at: track_model.created_at,
        artist_id: artist_model.id,
        artist_name: artist_model.name,
        artist_bio: artist_model.bio,
        artist_image: artist_model.image_url,
        artist_musicbrainz_id: artist_model.musicbrainz_id,
        album_id: album_model.as_ref().map(|a| a.id),
        album_title: album_model.as_ref().map(|a| a.title.clone()),
        album_cover_url: album_model.as_ref().and_then(|a| a.cover_url.clone()),
        album_genre: album_model.as_ref().and_then(|a| a.genre.clone()),
        album_year: album_model.as_ref().and_then(|a| a.year),
        album_musicbrainz_id: album_model.as_ref().and_then(|a| a.musicbrainz_id.clone()),
        best_bitrate: None,
        best_source: None,
    }))
}

// ─── Track Update (owner only) ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateTrackRequest {
    pub title: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
}

/// PUT /api/tracks/:id — update track metadata (owner only)
pub async fn update_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTrackRequest>,
) -> Result<Json<TrackResponse>, (StatusCode, String)> {
    let existing = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    // Only the uploader can edit
    match existing.uploaded_by {
        Some(uploader_id) if uploader_id == user.0.sub => {}
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                "You can only edit your own tracks".to_string(),
            ))
        }
    }

    let mut active: track::ActiveModel = existing.into();
    if let Some(title) = body.title {
        active.title = Set(title);
    }
    if let Some(genre) = body.genre {
        active.genre = Set(Some(genre));
    }
    if let Some(year) = body.year {
        active.year = Set(Some(year));
    }
    if let Some(track_number) = body.track_number {
        active.track_number = Set(Some(track_number));
    }
    if let Some(disc_number) = body.disc_number {
        active.disc_number = Set(Some(disc_number));
    }

    let updated = active
        .update(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let mut track = TrackResponse::from(updated);
    attach_availability(&state, [&mut track]).await?;
    Ok(Json(track))
}

/// DELETE /api/tracks/:id — withdraw a local publication while retaining playlist entries.
pub async fn delete_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let db_error = |e: sea_orm::DbErr| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let tx = crate::storage_catalog::begin_write(&state.db)
        .await
        .map_err(db_error)?;
    let track = track::Entity::find_by_id(id)
        .one(&tx)
        .await
        .map_err(db_error)?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".into()))?;
    if track.uploaded_by != Some(user.0.sub)
        || !crate::publications::local(&tx, id)
            .await
            .map_err(db_error)?
    {
        return Err((
            StatusCode::FORBIDDEN,
            "You can only withdraw your own publications".into(),
        ));
    }
    let change = crate::publications::remove(&tx, &state, &track, true)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    change.committed(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Explore: Popular Tracks ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExploreParams {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

/// GET /api/tracks/popular — tracks sorted by play_count DESC
pub async fn list_popular_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExploreParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = track::Entity::find()
        .filter(playable_filter(&state))
        .order_by_desc(track::Column::PlayCount)
        .order_by_asc(track::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

// ─── Explore: Trending Tracks ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TrendingParams {
    pub window: Option<String>,
    pub limit: Option<u64>,
}

/// GET /api/tracks/trending — time-windowed trending tracks.
///
/// Uses Redis sorted sets when available for real-time trending data,
/// falling back to PostgreSQL `listen_history` aggregation otherwise.
///
/// Query params:
/// - `window`: `"1h"`, `"24h"` (default), or `"7d"`
/// - `limit`: max tracks to return (default 20, max 100)
pub async fn list_trending_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TrendingParams>,
) -> Result<Json<Vec<TrackResponse>>, (StatusCode, String)> {
    use crate::trending::{self, TrendingWindow};

    let window = match params.window.as_deref() {
        Some("1h") => TrendingWindow::OneHour,
        Some("7d") => TrendingWindow::SevenDays,
        _ => TrendingWindow::TwentyFourHours, // default
    };
    let limit = params.limit.unwrap_or(20).min(100);

    // Try Redis first, fall back to PostgreSQL
    let entries = {
        #[cfg(feature = "redis")]
        {
            let redis_result = if let Some(ref pool) = state.redis {
                trending::fetch_trending_redis(pool, window, limit as usize).await
            } else {
                None
            };

            match redis_result {
                Some(entries) => entries,
                None => trending::fetch_trending_postgres(&state.db, window, limit)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?,
            }
        }

        #[cfg(not(feature = "redis"))]
        {
            trending::fetch_trending_postgres(&state.db, window, limit)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        }
    };

    if entries.is_empty() {
        return Ok(Json(vec![]));
    }

    // Fetch full track data for the trending IDs
    let track_ids: Vec<Uuid> = entries.iter().map(|e| e.track_id).collect();
    let tracks_map: HashMap<Uuid, track::Model> = track::Entity::find()
        .filter(playable_filter(&state))
        .filter(track::Column::Id.is_in(track_ids.clone()))
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .into_iter()
        .map(|t| (t.id, t))
        .collect();

    // Batch-fetch artists and albums
    let artist_ids: Vec<Uuid> = tracks_map
        .values()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks_map
        .values()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    // Preserve the trending order (by score desc)
    let data: Vec<TrackResponse> = track_ids
        .iter()
        .filter_map(|id| {
            let t = tracks_map.get(id)?;
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t.clone());
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            Some(resp)
        })
        .collect();

    Ok(Json(data))
}

// ─── Explore: Random Tracks ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RandomTracksParams {
    pub count: Option<u64>,
    pub genre: Option<String>,
}

/// GET /api/tracks/random — return random tracks, optionally filtered by genre
pub async fn list_random_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RandomTracksParams>,
) -> Result<Json<Vec<TrackResponse>>, (StatusCode, String)> {
    use sea_orm::QuerySelect;
    use sea_orm::{sea_query::Expr, Order};

    let count = params.count.unwrap_or(10).min(50);

    let mut query = track::Entity::find().filter(playable_filter(&state));
    if let Some(ref genre) = params.genre {
        query = query.filter(track::Column::Genre.eq(genre.clone()));
    }

    let selected: Vec<track::Model> = query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(count)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Batch-fetch artist/album data
    let artist_ids: Vec<Uuid> = selected
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = selected
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = selected
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(Json(data))
}

// ─── Explore: Recently Added Tracks ─────────────────────────────────

/// GET /api/tracks/recently-added — tracks sorted by created_at DESC
pub async fn list_recently_added_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = track::Entity::find()
        .filter(playable_filter(&state))
        .order_by_desc(track::Column::CreatedAt)
        .order_by_asc(track::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist and album data (same pattern as list_tracks)
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

// ─── Genres ─────────────────────────────────────────────────────────

/// GET /api/genres — list distinct genres
pub async fn list_genres(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    use sea_orm::{FromQueryResult, QuerySelect};

    #[derive(Debug, FromQueryResult)]
    struct GenreRow {
        genre: Option<String>,
    }

    let rows = track::Entity::find()
        .filter(playable_filter(&state))
        .select_only()
        .column(track::Column::Genre)
        .distinct()
        .into_model::<GenreRow>()
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let genres: Vec<String> = rows
        .into_iter()
        .filter_map(|r| r.genre)
        .filter(|g| !g.is_empty())
        .collect();

    Ok(Json(genres))
}

/// GET /api/genres/:genre/tracks — tracks filtered by genre
pub async fn list_genre_tracks(
    State(state): State<Arc<AppState>>,
    Path(genre): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    let paginator = track::Entity::find()
        .filter(playable_filter(&state))
        .filter(track::Column::Genre.eq(genre))
        .order_by_desc(track::Column::CreatedAt)
        .order_by_asc(track::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = TrackResponse::playable(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_track_model() -> track::Model {
        track::Model {
            id: Uuid::new_v4(),
            title: "Test Track".into(),
            artist_id: Uuid::new_v4(),
            album_id: Some(Uuid::new_v4()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_secs: 240.5,
            genre: Some("Rock".into()),
            year: Some(2024),
            musicbrainz_id: None,
            file_path: "/data/music/test.mp3".into(),
            file_size: 5_000_000,
            format: "mp3".into(),
            bitrate: Some(320),
            sample_rate: Some(44100),
            waveform_data: None,
            uploaded_by: Some(Uuid::new_v4()),
            play_count: 42,
            file_hash: None,
            content_hash: None,
            created_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_track_response_from_model() {
        let model = make_track_model();
        let id = model.id;
        let resp = TrackResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.title, "Test Track");
        assert_eq!(resp.play_count, 42);
        assert!(resp.artist_name.is_none());
        assert!(resp.album_title.is_none());
        assert!(resp.cover_url.is_none());
    }

    #[test]
    fn test_track_response_serialization() {
        let model = make_track_model();
        let resp = TrackResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["title"], "Test Track");
        assert_eq!(json["format"], "mp3");
        assert_eq!(json["play_count"], 42);
        // Optional None fields with skip_serializing_if should be absent
        assert!(json.get("artist_name").is_none());
        assert!(json.get("album_title").is_none());
    }

    #[test]
    fn test_paginated_response_serialization() {
        let resp = PaginatedResponse {
            data: vec!["a".to_string(), "b".to_string()],
            total: 10,
            page: 1,
            per_page: 2,
            total_pages: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 10);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 2);
        assert_eq!(json["total_pages"], 5);
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_pagination_params_defaults() {
        let params: PaginationParams = serde_json::from_str("{}").unwrap();
        assert!(params.page.is_none());
        assert!(params.per_page.is_none());
    }

    #[test]
    fn test_pagination_params_custom() {
        let params: PaginationParams =
            serde_json::from_str(r#"{"page": 3, "per_page": 50}"#).unwrap();
        assert_eq!(params.page, Some(3));
        assert_eq!(params.per_page, Some(50));
    }
}
