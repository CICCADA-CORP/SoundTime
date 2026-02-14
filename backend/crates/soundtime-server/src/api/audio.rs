use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use soundtime_audio::extract_metadata_from_file;
use soundtime_db::entities::{album, artist, remote_track, track};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use soundtime_db::AppState;

/// Extract p2p node from type-erased state
fn get_p2p_node(state: &AppState) -> Option<std::sync::Arc<soundtime_p2p::P2pNode>> {
    state
        .p2p
        .as_ref()
        .and_then(|any| any.clone().downcast::<soundtime_p2p::P2pNode>().ok())
}

/// SECURITY: Validate audio file magic bytes to prevent disguised file uploads
fn validate_audio_magic_bytes(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    // MP3: ID3 tag or sync frame
    if data.starts_with(b"ID3") || (data[0] == 0xFF && (data[1] & 0xE0) == 0xE0) {
        return true;
    }
    // FLAC
    if data.starts_with(b"fLaC") {
        return true;
    }
    // OGG (Vorbis/Opus)
    if data.starts_with(b"OggS") {
        return true;
    }
    // WAV (RIFF....WAVE)
    if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WAVE" {
        return true;
    }
    // AIFF
    if data.starts_with(b"FORM") && data.len() >= 12 && &data[8..12] == b"AIFF" {
        return true;
    }
    // AAC/M4A/MP4 (ftyp box)
    if data.len() >= 8 && &data[4..8] == b"ftyp" {
        return true;
    }
    // WMA/ASF
    if data.starts_with(&[0x30, 0x26, 0xB2, 0x75]) {
        return true;
    }
    false
}

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub id: Uuid,
    pub title: String,
    pub duration: f64,
    pub format: String,
    pub message: String,
}

/// POST /api/upload  — Multipart audio file upload
pub async fn upload_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user_id = user.0.sub;

    let mut file_data: Option<(String, Vec<u8>)> = None;
    let mut meta_title: Option<String> = None;
    let mut meta_album: Option<String> = None;
    let mut meta_artist: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("upload.mp3").to_string();
                let data = field.bytes().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("Read error: {e}") })),
                    )
                })?;
                file_data = Some((filename, data.to_vec()));
            }
            "title" => {
                meta_title = field.text().await.ok();
            }
            "album" => {
                meta_album = field.text().await.ok();
            }
            "artist" => {
                meta_artist = field.text().await.ok();
            }
            _ => {}
        }
    }

    let (filename, data) = file_data.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No file provided" })),
        )
    })?;

    // Validate format
    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if !soundtime_audio::metadata::is_supported_format(&ext) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("Unsupported format: {ext}") })),
        ));
    }

    // SECURITY: validate audio magic bytes to prevent disguised file uploads
    if !validate_audio_magic_bytes(&data) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "File content does not match a recognized audio format" }),
            ),
        ));
    }

    // Store file
    let album_name = meta_album.as_deref();
    let relative_path = state
        .storage
        .store_file(user_id, album_name, &filename, &data)
        .await
        .map_err(|e| {
            tracing::error!("Storage error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to store file" })),
            )
        })?;

    // Extract metadata from stored file
    let full_path = soundtime_audio::ensure_local_file(state.storage.as_ref(), &relative_path)
        .await
        .map_err(|e| {
            tracing::error!("ensure_local_file error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to access file locally" })),
            )
        })?;

    // Convert AIFF → FLAC if necessary
    let (full_path, relative_path) = if soundtime_audio::needs_aiff_conversion(&ext) {
        let flac_path = soundtime_audio::convert_aiff_to_flac(&full_path)
            .await
            .map_err(|e| {
                tracing::error!("AIFF→FLAC conversion error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("AIFF conversion failed: {e}") })),
                )
            })?;
        // Update relative path: replace .aif/.aiff extension with .flac
        let new_relative = relative_path
            .rsplit_once('.')
            .map(|(base, _)| format!("{base}.flac"))
            .unwrap_or_else(|| relative_path.clone());
        (flac_path, new_relative)
    } else {
        (full_path, relative_path)
    };

    let audio_meta = extract_metadata_from_file(&full_path).map_err(|e| {
        tracing::error!("Metadata extraction error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to extract metadata" })),
        )
    })?;

    // Generate waveform
    let waveform = soundtime_audio::generate_waveform(&full_path, 200).ok();

    // Resolve or create artist
    let artist_name = meta_artist
        .clone()
        .or(audio_meta.artist.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let existing_artist = artist::Entity::find()
        .filter(artist::Column::Name.eq(&artist_name))
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let artist_id = if let Some(a) = existing_artist {
        a.id
    } else {
        let new_artist = artist::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(artist_name.clone()),
            bio: Set(None),
            image_url: Set(None),
            musicbrainz_id: Set(None),
            created_at: Set(chrono::Utc::now().into()),
        };
        let result = new_artist.insert(&state.db).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;
        result.id
    };

    // Resolve album artist: prefer album_artist tag, fall back to track artist
    let album_artist_name = audio_meta
        .album_artist
        .clone()
        .or(meta_artist.clone())
        .or(audio_meta.artist.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let album_artist_id = if album_artist_name == artist_name {
        // Same as track artist — reuse the ID
        artist_id
    } else {
        let existing = artist::Entity::find()
            .filter(artist::Column::Name.eq(&album_artist_name))
            .one(&state.db)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                )
            })?;
        if let Some(a) = existing {
            a.id
        } else {
            let new_artist = artist::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(album_artist_name.clone()),
                bio: Set(None),
                image_url: Set(None),
                musicbrainz_id: Set(None),
                created_at: Set(chrono::Utc::now().into()),
            };
            new_artist
                .insert(&state.db)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                    )
                })?
                .id
        }
    };

    // Resolve or create album
    let album_title = meta_album
        .or(audio_meta.album.clone())
        .unwrap_or_else(|| "Singles".to_string());

    let existing_album = album::Entity::find()
        .filter(album::Column::Title.eq(&album_title))
        .filter(album::Column::ArtistId.eq(album_artist_id))
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let album_id = if let Some(a) = existing_album {
        a.id
    } else {
        let new_album = album::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(album_title.clone()),
            artist_id: Set(album_artist_id),
            release_date: Set(None),
            cover_url: Set(None),
            musicbrainz_id: Set(None),
            genre: Set(audio_meta.genre.clone()),
            year: Set(audio_meta.year.map(|y| y as i16)),
            created_at: Set(chrono::Utc::now().into()),
        };
        let result = new_album.insert(&state.db).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

        // Store cover art if present
        if let Some(cover_data) = &audio_meta.cover_art {
            if let Ok(cover_path) = state
                .storage
                .store_cover(user_id, Some(&album_title), cover_data)
                .await
            {
                let mut update: album::ActiveModel = result.clone().into();
                update.cover_url = Set(Some(format!("/api/media/{cover_path}")));
                let _ = update.update(&state.db).await;
            }
        }

        result.id
    };

    // Create track
    let track_title = meta_title.or(audio_meta.title.clone()).unwrap_or_else(|| {
        std::path::Path::new(&filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    });

    let track_id = Uuid::new_v4();
    let new_track = track::ActiveModel {
        id: Set(track_id),
        title: Set(track_title.clone()),
        album_id: Set(Some(album_id)),
        artist_id: Set(artist_id),
        track_number: Set(audio_meta.track_number.map(|n| n as i16)),
        disc_number: Set(audio_meta.disc_number.map(|n| n as i16)),
        duration_secs: Set(audio_meta.duration_secs as f32),
        genre: Set(audio_meta.genre.clone()),
        year: Set(audio_meta.year.map(|y| y as i16)),
        musicbrainz_id: Set(None),
        file_path: Set(relative_path),
        format: Set(audio_meta.format.clone()),
        file_size: Set(audio_meta.file_size as i64),
        bitrate: Set(audio_meta.bitrate.map(|b| b as i32)),
        sample_rate: Set(audio_meta.sample_rate.map(|s| s as i32)),
        waveform_data: Set(waveform.map(|w| serde_json::json!(w))),
        uploaded_by: Set(Some(user_id)),
        content_hash: Set(None),
        play_count: Set(0),
        created_at: Set(chrono::Utc::now().into()),
    };

    new_track.insert(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    // Publish track to P2P blob store (best-effort)
    publish_track_to_p2p(
        &state,
        track_id,
        &data,
        &track_title,
        &artist_name,
        &album_title,
        &audio_meta,
    )
    .await;

    // Dispatch plugin event (best-effort)
    if let Some(registry) = super::get_plugin_registry(&state) {
        let payload = soundtime_plugin::TrackAddedPayload {
            track_id: track_id.to_string(),
            title: track_title.clone(),
            artist: artist_name.clone(),
            album: Some(album_title.clone()),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_track_added", &payload_val).await;
        });
    }

    Ok(Json(UploadResponse {
        id: track_id,
        title: track_title,
        duration: audio_meta.duration_secs,
        format: audio_meta.format,
        message: "Track uploaded successfully".into(),
    }))
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct StreamParams {
    pub _format: Option<String>,
}

/// GET /api/tracks/:id/stream — Stream audio with Range support
pub async fn stream_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let track_record = track::Entity::find_by_id(id)
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
                Json(serde_json::json!({ "error": "Track not found" })),
            )
        })?;

    let file_path_str = &track_record.file_path;

    // Determine content type from format
    let content_type = match track_record.format.as_str() {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        "opus" => "audio/opus",
        "aiff" => "audio/aiff",
        _ => "application/octet-stream",
    };

    // Parse Range header
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_range_header);

    // P2P tracks: file_path starts with "p2p://<hash>"
    if let Some(hash_str) = file_path_str.strip_prefix("p2p://") {
        let p2p_node = get_p2p_node(&state).ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "P2P node not available" })),
            )
        })?;

        // FIX-16: Check if the remote track is marked unavailable before attempting fetch
        let remote_record = remote_track::Entity::find()
            .filter(remote_track::Column::RemoteUri.ends_with(format!("/{hash_str}")))
            .one(&state.db)
            .await
            .map_err(|e| {
                tracing::error!("DB error looking up remote track: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                )
            })?;

        if let Some(ref rt) = remote_record {
            if !rt.is_available {
                tracing::warn!(%hash_str, "P2P track marked unavailable, returning 410 Gone");
                return Err((
                    StatusCode::GONE,
                    Json(serde_json::json!({ "error": "P2P track is no longer available" })),
                ));
            }
        }

        let hash: soundtime_p2p::BlobHash = hash_str.parse().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid P2P hash" })),
            )
        })?;

        // FIX-15: When fetch fails, trigger auto_repair_on_failure for health tracking
        let data = match p2p_node.get_or_fetch_track(hash).await {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!(%hash, error = %e, "failed to fetch P2P track");

                // Determine origin node from the remote_track record
                let origin_node = remote_record
                    .as_ref()
                    .map(|rt| {
                        rt.instance_domain
                            .strip_prefix("p2p://")
                            .unwrap_or(&rt.instance_domain)
                            .to_string()
                    })
                    .unwrap_or_default();

                if !origin_node.is_empty() {
                    let health_manager = Arc::clone(p2p_node.health_manager());
                    let p2p_clone = Arc::clone(&p2p_node);
                    let hash_string = hash_str.to_string();
                    tokio::spawn(async move {
                        soundtime_p2p::auto_repair_on_failure(
                            &health_manager,
                            &p2p_clone,
                            &hash_string,
                            &origin_node,
                        )
                        .await;
                    });
                }

                return Err((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "P2P track data not available" })),
                ));
            }
        };

        let file_size = data.len() as u64;

        let (start, end) = match range {
            Some((s, e)) => {
                let end = e.unwrap_or(file_size - 1).min(file_size - 1);
                (s, end)
            }
            None => (0, file_size - 1),
        };

        let content_length = end - start + 1;
        let slice = data.slice(start as usize..(start + content_length) as usize);
        let body = Body::from(slice);

        let status = if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };

        let mut response_headers = HeaderMap::new();
        response_headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        response_headers.insert(
            header::CONTENT_LENGTH,
            content_length.to_string().parse().unwrap(),
        );
        response_headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());

        if range.is_some() {
            response_headers.insert(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{file_size}").parse().unwrap(),
            );
        }

        return Ok((status, response_headers, body));
    }

    // Regular filesystem track
    let file_path = soundtime_audio::ensure_local_file(state.storage.as_ref(), file_path_str)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Audio file not found" })),
            )
        })?;

    let file_size = tokio::fs::metadata(&file_path)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Cannot read file" })),
            )
        })?
        .len();

    let (start, end) = match range {
        Some((s, e)) => {
            let end = e.unwrap_or(file_size - 1).min(file_size - 1);
            (s, end)
        }
        None => (0, file_size - 1),
    };

    let content_length = end - start + 1;

    let mut file = tokio::fs::File::open(&file_path).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Cannot open file" })),
        )
    })?;

    if start > 0 {
        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "Seek error" })),
                )
            })?;
    }

    let limited = file.take(content_length);
    let stream = ReaderStream::new(limited);
    let body = Body::from_stream(stream);

    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    response_headers.insert(
        header::CONTENT_LENGTH,
        content_length.to_string().parse().unwrap(),
    );
    response_headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());

    if range.is_some() {
        response_headers.insert(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{file_size}").parse().unwrap(),
        );
    }

    Ok((status, response_headers, body))
}

/// Parse "bytes=start-end" range header
fn parse_range_header(header: &str) -> Option<(u64, Option<u64>)> {
    let range = header.strip_prefix("bytes=")?;
    let mut parts = range.splitn(2, '-');
    let start: u64 = parts.next()?.parse().ok()?;
    let end: Option<u64> = parts
        .next()
        .and_then(|s| if s.is_empty() { None } else { s.parse().ok() });
    Some((start, end))
}

/// GET /api/media/*path — Serve static media files (covers, etc.)
pub async fn serve_media(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    // Try the primary storage path first
    let base_path = state.storage.full_path("");
    let file_path = state.storage.full_path(&path);

    let resolved = try_resolve_media(&base_path, &file_path);

    // If not found and METADATA_STORAGE_PATH is set, try there
    let resolved = match resolved {
        Some(r) => r,
        None => {
            if let Ok(meta_base) = std::env::var("METADATA_STORAGE_PATH") {
                let meta_base_path = std::path::PathBuf::from(&meta_base);
                let meta_file_path = meta_base_path.join(&path);
                try_resolve_media(&meta_base_path, &meta_file_path).ok_or(StatusCode::NOT_FOUND)?
            } else {
                return Err(StatusCode::NOT_FOUND);
            }
        }
    };

    let data = tokio::fs::read(&resolved)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let content_type = match resolved.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=31536000, immutable".parse().unwrap(),
    );

    Ok((headers, data))
}

/// SECURITY: Resolve a media file path, ensuring it stays within the base directory.
/// Returns `None` if the file doesn't exist or would escape the base.
fn try_resolve_media(
    base_path: &std::path::Path,
    file_path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let canonical = file_path.canonicalize().ok()?;
    let base_canonical = base_path.canonicalize().ok()?;
    if !canonical.starts_with(&base_canonical) {
        return None; // path traversal attempt
    }
    if !canonical.is_file() {
        return None;
    }
    Some(canonical)
}

// ─── Multi-file batch upload ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BatchUploadResponse {
    pub results: Vec<BatchUploadItem>,
    pub total: usize,
    pub success: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct BatchUploadItem {
    pub filename: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<UploadResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// POST /api/upload/batch — Upload multiple audio files at once.
/// Each file is sent as a multipart field named "files".
pub async fn upload_tracks_batch(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<Json<BatchUploadResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user_id = user.0.sub;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    const MAX_BATCH_FILES: usize = 50;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "files" || name == "file" {
            let filename = field.file_name().unwrap_or("upload.mp3").to_string();
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Read error: {e}") })),
                )
            })?;
            files.push((filename, data.to_vec()));
            if files.len() > MAX_BATCH_FILES {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({ "error": format!("Too many files. Maximum is {MAX_BATCH_FILES}") }),
                    ),
                ));
            }
        }
    }

    if files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No files provided" })),
        ));
    }

    let total = files.len();
    let mut results = Vec::with_capacity(total);
    let mut success_count = 0usize;

    for (filename, data) in files {
        match process_single_upload(&state, user_id, &filename, &data).await {
            Ok(resp) => {
                success_count += 1;
                results.push(BatchUploadItem {
                    filename,
                    success: true,
                    track: Some(resp),
                    error: None,
                });
            }
            Err(e) => {
                results.push(BatchUploadItem {
                    filename,
                    success: false,
                    track: None,
                    error: Some(e),
                });
            }
        }
    }

    Ok(Json(BatchUploadResponse {
        total,
        success: success_count,
        failed: total - success_count,
        results,
    }))
}

/// Shared logic for processing a single file upload (used by both single and batch).
async fn process_single_upload(
    state: &AppState,
    user_id: Uuid,
    filename: &str,
    data: &[u8],
) -> Result<UploadResponse, String> {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if !soundtime_audio::metadata::is_supported_format(&ext) {
        return Err(format!("Unsupported format: {ext}"));
    }

    // SECURITY: validate audio magic bytes
    if !validate_audio_magic_bytes(data) {
        return Err("File content does not match a recognized audio format".to_string());
    }

    let relative_path = state
        .storage
        .store_file(user_id, None, filename, data)
        .await
        .map_err(|e| format!("Storage error: {e}"))?;

    let full_path = soundtime_audio::ensure_local_file(state.storage.as_ref(), &relative_path)
        .await
        .map_err(|e| format!("Local file: {e}"))?;

    // Convert AIFF → FLAC if necessary
    let (full_path, relative_path) = if soundtime_audio::needs_aiff_conversion(&ext) {
        let flac_path = soundtime_audio::convert_aiff_to_flac(&full_path)
            .await
            .map_err(|e| format!("AIFF→FLAC conversion: {e}"))?;
        let new_relative = relative_path
            .rsplit_once('.')
            .map(|(base, _)| format!("{base}.flac"))
            .unwrap_or_else(|| relative_path.clone());
        (flac_path, new_relative)
    } else {
        (full_path, relative_path)
    };

    let audio_meta =
        extract_metadata_from_file(&full_path).map_err(|e| format!("Metadata: {e}"))?;

    let waveform = soundtime_audio::generate_waveform(&full_path, 200).ok();

    let artist_name = audio_meta
        .artist
        .clone()
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let existing_artist = artist::Entity::find()
        .filter(artist::Column::Name.eq(&artist_name))
        .one(&state.db)
        .await
        .map_err(|e| format!("DB: {e}"))?;

    let artist_id = if let Some(a) = existing_artist {
        a.id
    } else {
        let new_artist = artist::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(artist_name.clone()),
            bio: Set(None),
            image_url: Set(None),
            musicbrainz_id: Set(None),
            created_at: Set(chrono::Utc::now().into()),
        };
        new_artist
            .insert(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?
            .id
    };

    // Resolve album artist: prefer album_artist tag, fall back to track artist
    let album_artist_name = audio_meta
        .album_artist
        .clone()
        .unwrap_or_else(|| artist_name.clone());

    let album_artist_id = if album_artist_name == artist_name {
        artist_id
    } else {
        let existing = artist::Entity::find()
            .filter(artist::Column::Name.eq(&album_artist_name))
            .one(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?;
        if let Some(a) = existing {
            a.id
        } else {
            let new_artist = artist::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(album_artist_name.clone()),
                bio: Set(None),
                image_url: Set(None),
                musicbrainz_id: Set(None),
                created_at: Set(chrono::Utc::now().into()),
            };
            new_artist
                .insert(&state.db)
                .await
                .map_err(|e| format!("DB: {e}"))?
                .id
        }
    };

    let album_title = audio_meta
        .album
        .clone()
        .unwrap_or_else(|| "Singles".to_string());

    let existing_album = album::Entity::find()
        .filter(album::Column::Title.eq(&album_title))
        .filter(album::Column::ArtistId.eq(album_artist_id))
        .one(&state.db)
        .await
        .map_err(|e| format!("DB: {e}"))?;

    let album_id = if let Some(a) = existing_album {
        a.id
    } else {
        let new_album = album::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(album_title.clone()),
            artist_id: Set(album_artist_id),
            release_date: Set(None),
            cover_url: Set(None),
            musicbrainz_id: Set(None),
            genre: Set(audio_meta.genre.clone()),
            year: Set(audio_meta.year.map(|y| y as i16)),
            created_at: Set(chrono::Utc::now().into()),
        };
        let result = new_album
            .insert(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?;

        // Store cover art if present in metadata
        if let Some(cover_data) = &audio_meta.cover_art {
            if let Ok(cover_path) = state
                .storage
                .store_cover(user_id, Some(&album_title), cover_data)
                .await
            {
                let mut update: album::ActiveModel = result.clone().into();
                update.cover_url = Set(Some(format!("/api/media/{cover_path}")));
                let _ = update.update(&state.db).await;
            }
        }

        result.id
    };

    let track_title = audio_meta.title.clone().unwrap_or_else(|| {
        std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    });

    let track_id = Uuid::new_v4();
    let new_track = track::ActiveModel {
        id: Set(track_id),
        title: Set(track_title.clone()),
        album_id: Set(Some(album_id)),
        artist_id: Set(artist_id),
        track_number: Set(audio_meta.track_number.map(|n| n as i16)),
        disc_number: Set(audio_meta.disc_number.map(|n| n as i16)),
        duration_secs: Set(audio_meta.duration_secs as f32),
        genre: Set(audio_meta.genre.clone()),
        year: Set(audio_meta.year.map(|y| y as i16)),
        musicbrainz_id: Set(None),
        file_path: Set(relative_path),
        format: Set(audio_meta.format.clone()),
        file_size: Set(audio_meta.file_size as i64),
        bitrate: Set(audio_meta.bitrate.map(|b| b as i32)),
        sample_rate: Set(audio_meta.sample_rate.map(|s| s as i32)),
        waveform_data: Set(waveform.map(|w| serde_json::json!(w))),
        uploaded_by: Set(Some(user_id)),
        content_hash: Set(None),
        play_count: Set(0),
        created_at: Set(chrono::Utc::now().into()),
    };

    new_track
        .insert(&state.db)
        .await
        .map_err(|e| format!("DB: {e}"))?;

    // Publish track to P2P blob store (best-effort)
    publish_track_to_p2p(
        state,
        track_id,
        data,
        &track_title,
        &artist_name,
        &album_title,
        &audio_meta,
    )
    .await;

    // Dispatch plugin event (best-effort)
    if let Some(registry) = super::get_plugin_registry(state) {
        let payload = soundtime_plugin::TrackAddedPayload {
            track_id: track_id.to_string(),
            title: track_title.clone(),
            artist: artist_name.clone(),
            album: Some(album_title.clone()),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_track_added", &payload_val).await;
        });
    }

    Ok(UploadResponse {
        id: track_id,
        title: track_title,
        duration: audio_meta.duration_secs,
        format: audio_meta.format,
        message: "Track uploaded successfully".into(),
    })
}

// ─── P2P publication helper ─────────────────────────────────────────

/// Publish a newly uploaded track to the P2P blob store and broadcast
/// an announcement to all connected peers.
///
/// This is best-effort: failures are logged but do not prevent the upload
/// from succeeding. Called from both single and batch upload paths.
async fn publish_track_to_p2p(
    state: &AppState,
    track_id: Uuid,
    data: &[u8],
    track_title: &str,
    artist_name: &str,
    album_title: &str,
    audio_meta: &soundtime_audio::AudioMetadata,
) {
    let p2p = match get_p2p_node(state) {
        Some(p2p) => p2p,
        None => return,
    };

    let data_bytes = bytes::Bytes::from(data.to_vec());
    match p2p.publish_track(data_bytes).await {
        Ok(hash) => {
            tracing::info!(%track_id, %hash, "track published to P2P blob store");
            // Update the content_hash in the DB
            let update = track::ActiveModel {
                id: Set(track_id),
                content_hash: Set(Some(hash.to_string())),
                ..Default::default()
            };
            if let Err(e) = update.update(&state.db).await {
                tracing::warn!(%track_id, "failed to save content_hash: {e}");
            }

            // Publish cover art to P2P blob store if available
            let cover_hash = if let Some(cover_data) = &audio_meta.cover_art {
                match p2p
                    .publish_cover(bytes::Bytes::from(cover_data.clone()))
                    .await
                {
                    Ok(h) => Some(h.to_string()),
                    Err(e) => {
                        tracing::warn!(%track_id, "failed to publish cover to P2P: {e}");
                        None
                    }
                }
            } else {
                None
            };

            // Broadcast full track metadata to all connected peers
            let announcement = soundtime_p2p::TrackAnnouncement {
                hash: hash.to_string(),
                title: track_title.to_string(),
                artist_name: artist_name.to_string(),
                album_artist_name: audio_meta.album_artist.clone(),
                album_title: Some(album_title.to_string()),
                duration_secs: audio_meta.duration_secs as f32,
                format: audio_meta.format.clone(),
                file_size: audio_meta.file_size as i64,
                genre: audio_meta.genre.clone(),
                year: audio_meta.year.map(|y| y as i16),
                track_number: audio_meta.track_number.map(|n| n as i16),
                disc_number: audio_meta.disc_number.map(|n| n as i16),
                bitrate: audio_meta.bitrate.map(|b| b as i32),
                sample_rate: audio_meta.sample_rate.map(|s| s as i32),
                origin_node: p2p.node_id().to_string(),
                cover_hash,
            };
            let p2p_clone = Arc::clone(&p2p);
            tokio::spawn(async move {
                p2p_clone.broadcast_announce_track(announcement).await;
            });
        }
        Err(e) => {
            tracing::warn!(%track_id, "failed to publish track to P2P: {e}");
        }
    }
}

// ─── Album Cover Upload ────────────────────────────────────────────

/// POST /api/albums/:id/cover — Upload a custom cover image for an album.
/// Only the user who uploaded tracks to this album can update the cover.
pub async fn upload_album_cover(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(album_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let user_id = user.0.sub;

    // Verify album exists
    let album_record = album::Entity::find_by_id(album_id)
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
                Json(serde_json::json!({ "error": "Album not found" })),
            )
        })?;

    // Verify this user uploaded at least one track in this album
    let user_track = track::Entity::find()
        .filter(track::Column::AlbumId.eq(Some(album_id)))
        .filter(track::Column::UploadedBy.eq(Some(user_id)))
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    // Also allow admins
    let is_admin = {
        use soundtime_db::entities::user::{self, UserRole};
        user::Entity::find_by_id(user_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|u| u.role == UserRole::Admin)
            .unwrap_or(false)
    };

    if user_track.is_none() && !is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({ "error": "Only the uploader or an admin can change album cover" }),
            ),
        ));
    }

    // Read the cover image from multipart
    let mut cover_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "cover" || name == "file" {
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Read error: {e}") })),
                )
            })?;

            // Validate image (check first bytes for JPEG/PNG/WebP magic)
            if data.len() < 4 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "File too small to be a valid image" })),
                ));
            }

            let valid = data.starts_with(&[0xFF, 0xD8, 0xFF]) // JPEG
                || data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) // PNG
                || (data.len() > 12 && &data[8..12] == b"WEBP"); // WebP

            if !valid {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({ "error": "Invalid image format (JPEG, PNG, or WebP only)" }),
                    ),
                ));
            }

            // Limit to 10MB
            if data.len() > 10 * 1024 * 1024 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Cover image exceeds 10MB limit" })),
                ));
            }

            cover_data = Some(data.to_vec());
        }
    }

    let data = cover_data.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No cover image provided" })),
        )
    })?;

    // Store the cover
    let cover_path = state
        .storage
        .store_cover(user_id, Some(&album_record.title), &data)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Store cover: {e}") })),
            )
        })?;

    // Update album record
    let cover_url = format!("/api/media/{cover_path}");
    let mut update: album::ActiveModel = album_record.into();
    update.cover_url = Set(Some(cover_url.clone()));
    update.update(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "message": "Cover updated successfully",
        "cover_url": cover_url
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── validate_audio_magic_bytes tests ──────────────────────────

    #[test]
    fn test_magic_bytes_mp3_id3() {
        let mut data = b"ID3".to_vec();
        data.extend_from_slice(&[0u8; 12]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_mp3_sync() {
        // MP3 sync frame: 0xFF followed by byte with top 3 bits set (0xE0 mask)
        let mut data = vec![0xFF, 0xE0];
        data.extend_from_slice(&[0u8; 12]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_flac() {
        let mut data = b"fLaC".to_vec();
        data.extend_from_slice(&[0u8; 12]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_ogg() {
        let mut data = b"OggS".to_vec();
        data.extend_from_slice(&[0u8; 12]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_wav() {
        // WAV: "RIFF" + 4 arbitrary bytes + "WAVE"
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        data.extend_from_slice(b"WAVE");
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_aiff() {
        // AIFF: "FORM" + 4 arbitrary bytes + "AIFF"
        let mut data = b"FORM".to_vec();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        data.extend_from_slice(b"AIFF");
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_aac_m4a() {
        // AAC/M4A: 4 bytes + "ftyp" + padding
        let mut data = vec![0x00, 0x00, 0x00, 0x20]; // size bytes
        data.extend_from_slice(b"ftyp");
        data.extend_from_slice(&[0u8; 8]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_wma() {
        // WMA/ASF: starts with [0x30, 0x26, 0xB2, 0x75]
        let mut data = vec![0x30, 0x26, 0xB2, 0x75];
        data.extend_from_slice(&[0u8; 12]);
        assert!(validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_too_short() {
        // Only 2 bytes — below the 12-byte minimum
        let data = b"ID";
        assert!(!validate_audio_magic_bytes(data));
    }

    #[test]
    fn test_magic_bytes_random() {
        let data = vec![
            0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44,
        ];
        assert!(!validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_elf() {
        // ELF binary magic: 0x7F "ELF"
        let mut data = vec![0x7F];
        data.extend_from_slice(b"ELF");
        data.extend_from_slice(&[0u8; 12]);
        assert!(!validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_pdf() {
        // PDF magic: "%PDF"
        let mut data = b"%PDF-1.4".to_vec();
        data.extend_from_slice(&[0u8; 8]);
        assert!(!validate_audio_magic_bytes(&data));
    }

    #[test]
    fn test_magic_bytes_empty() {
        let data: &[u8] = &[];
        assert!(!validate_audio_magic_bytes(data));
    }

    // ─── parse_range_header tests ──────────────────────────────────

    #[test]
    fn test_parse_range_open_end() {
        let result = parse_range_header("bytes=0-");
        assert_eq!(result, Some((0, None)));
    }

    #[test]
    fn test_parse_range_full() {
        let result = parse_range_header("bytes=100-200");
        assert_eq!(result, Some((100, Some(200))));
    }

    #[test]
    fn test_parse_range_from_start() {
        let result = parse_range_header("bytes=0-499");
        assert_eq!(result, Some((0, Some(499))));
    }

    #[test]
    fn test_parse_range_no_prefix() {
        let result = parse_range_header("0-100");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_range_invalid() {
        let result = parse_range_header("invalid");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_range_empty() {
        let result = parse_range_header("");
        assert_eq!(result, None);
    }

    // ─── DTO serialization tests ───────────────────────────────────

    #[test]
    fn test_upload_response_serialize() {
        let resp = UploadResponse {
            id: Uuid::nil(),
            title: "My Song".to_string(),
            duration: 180.5,
            format: "mp3".to_string(),
            message: "Track uploaded successfully".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["title"], "My Song");
        assert_eq!(json["duration"], 180.5);
        assert_eq!(json["format"], "mp3");
        assert_eq!(json["message"], "Track uploaded successfully");
        assert!(json["id"].is_string());
    }

    #[test]
    fn test_batch_upload_response_serialize() {
        let resp = BatchUploadResponse {
            results: vec![
                BatchUploadItem {
                    filename: "song.mp3".to_string(),
                    success: true,
                    track: Some(UploadResponse {
                        id: Uuid::nil(),
                        title: "Song".to_string(),
                        duration: 120.0,
                        format: "mp3".to_string(),
                        message: "ok".to_string(),
                    }),
                    error: None,
                },
                BatchUploadItem {
                    filename: "bad.exe".to_string(),
                    success: false,
                    track: None,
                    error: Some("Unsupported format".to_string()),
                },
            ],
            total: 2,
            success: 1,
            failed: 1,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 2);
        assert_eq!(json["success"], 1);
        assert_eq!(json["failed"], 1);
        assert_eq!(json["results"].as_array().unwrap().len(), 2);
        assert_eq!(json["results"][0]["filename"], "song.mp3");
        assert!(json["results"][0]["success"].as_bool().unwrap());
        assert_eq!(json["results"][1]["filename"], "bad.exe");
        assert!(!json["results"][1]["success"].as_bool().unwrap());
    }

    #[test]
    fn test_batch_upload_item_skip_none() {
        let item = BatchUploadItem {
            filename: "test.flac".to_string(),
            success: true,
            track: None,
            error: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        // Fields with skip_serializing_if = "Option::is_none" should be absent
        assert!(!json.as_object().unwrap().contains_key("track"));
        assert!(!json.as_object().unwrap().contains_key("error"));
        assert_eq!(json["filename"], "test.flac");
        assert!(json["success"].as_bool().unwrap());
    }
}
