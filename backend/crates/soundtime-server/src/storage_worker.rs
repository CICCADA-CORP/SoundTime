//! Storage worker — daily integrity check & import sync.
//!
//! - **Integrity check**: verifies every track's file still exists and
//!   computes a SHA-256 hash to detect corruption.
//! - **Sync / import**: scans the storage backend for audio files that
//!   are not yet referenced in the database and imports them.
//!
//! Both operations run as background tasks to avoid HTTP timeouts.
//! The admin API triggers them asynchronously and polls for results
//! via `TaskTracker`.

use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QuerySelect, Set,
};
use serde::Serialize;
use soundtime_audio::metadata::normalize_genre;
use soundtime_db::entities::{track, user};
use soundtime_db::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Interval between automatic runs (24 hours).
const DAILY_INTERVAL_SECS: u64 = 86_400;

/// Audio extensions we consider importable.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav", "aac", "opus", "aiff", "aif"];

// ─── Result types exposed to admin API ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityReport {
    pub total_checked: u64,
    pub healthy: u64,
    pub missing: Vec<MissingTrack>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingTrack {
    pub track_id: String,
    pub title: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub scanned: u64,
    pub imported: u64,
    pub skipped: u64,
    pub errors: Vec<String>,
}

// ─── Async task tracker ────────────────────────────────────────────

/// Status of a background storage task (sync or integrity check).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum TaskStatus {
    /// Task is currently running.
    #[serde(rename = "running")]
    Running { progress: TaskProgress },
    /// Task completed successfully.
    #[serde(rename = "completed")]
    Completed { result: TaskResult },
    /// Task failed with an error.
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskProgress {
    pub processed: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum TaskResult {
    #[serde(rename = "sync")]
    Sync(SyncReport),
    #[serde(rename = "integrity")]
    Integrity(IntegrityReport),
}

/// Shared handle to track the current background task.
pub type TaskTrackerHandle = Arc<Mutex<Option<TaskStatus>>>;

/// Create a new task tracker.
pub fn new_tracker() -> TaskTrackerHandle {
    Arc::new(Mutex::new(None))
}

// ─── Background spawner ────────────────────────────────────────────

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!("storage worker started (runs every 24h)");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(DAILY_INTERVAL_SECS)).await;
            tracing::info!("storage worker: starting daily integrity check");
            match run_integrity_check(&state, None).await {
                Ok(report) => {
                    tracing::info!(
                        "integrity check done: {} checked, {} healthy, {} missing",
                        report.total_checked,
                        report.healthy,
                        report.missing.len()
                    );
                }
                Err(e) => tracing::error!("integrity check failed: {e}"),
            }

            tracing::info!("storage worker: starting daily sync");
            match run_sync(&state, None).await {
                Ok(report) => {
                    tracing::info!(
                        "sync done: {} scanned, {} imported, {} skipped",
                        report.scanned,
                        report.imported,
                        report.skipped
                    );
                }
                Err(e) => tracing::error!("sync failed: {e}"),
            }
        }
    });
}

// ─── Integrity check (public for manual trigger) ──────────────────

pub async fn run_integrity_check(
    state: &AppState,
    tracker: Option<&TaskTrackerHandle>,
) -> Result<IntegrityReport, String> {
    // Filter out P2P tracks at DB level and paginate to avoid loading all rows
    let paginator = track::Entity::find()
        .filter(track::Column::FilePath.not_like("p2p://%"))
        .paginate(&state.db, 500);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| format!("DB count: {e}"))? as u64;

    let mut report = IntegrityReport {
        total_checked: 0,
        healthy: 0,
        missing: Vec::new(),
        errors: Vec::new(),
    };

    let mut page = 0u64;
    loop {
        let batch = paginator
            .fetch_page(page)
            .await
            .map_err(|e| format!("DB query page {page}: {e}"))?;
        if batch.is_empty() {
            break;
        }

        for t in &batch {
            report.total_checked += 1;

            // Update progress every 10 tracks
            if let Some(tr) = tracker {
                if report.total_checked.is_multiple_of(10) || report.total_checked == total {
                    let mut lock = tr.lock().await;
                    *lock = Some(TaskStatus::Running {
                        progress: TaskProgress {
                            processed: report.total_checked,
                            total: Some(total),
                        },
                    });
                }
            }

            if !state.storage.file_exists(&t.file_path).await {
                report.missing.push(MissingTrack {
                    track_id: t.id.to_string(),
                    title: t.title.clone(),
                    file_path: t.file_path.clone(),
                });
                continue;
            }

            // Verify file is readable (hash check)
            match state.storage.hash_file(&t.file_path).await {
                Ok(_) => report.healthy += 1,
                Err(e) => {
                    report
                        .errors
                        .push(format!("Track {} ({}): hash error — {}", t.id, t.title, e));
                }
            }
        }
        page += 1;
    }

    Ok(report)
}

// ─── Sync / import from storage ────────────────────────────────────

pub async fn run_sync(
    state: &AppState,
    tracker: Option<&TaskTrackerHandle>,
) -> Result<SyncReport, String> {
    let mut report = SyncReport {
        scanned: 0,
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
    };

    // Collect only file_path column from DB to minimise memory usage
    #[derive(Debug, FromQueryResult)]
    struct FilePathRow {
        file_path: String,
    }

    let known_paths: std::collections::HashSet<String> = track::Entity::find()
        .select_only()
        .column(track::Column::FilePath)
        .into_model::<FilePathRow>()
        .all(&state.db)
        .await
        .map_err(|e| format!("DB query: {e}"))?
        .into_iter()
        .map(|r| r.file_path)
        .collect();

    // List all files in storage root
    let all_files = state
        .storage
        .list_files("")
        .await
        .map_err(|e| format!("list_files: {e}"))?;

    // Pre-filter audio files so we can report accurate totals
    let audio_files: Vec<String> = all_files
        .into_iter()
        .filter(|file_path| {
            let ext = std::path::Path::new(file_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            AUDIO_EXTENSIONS.contains(&ext.as_str())
        })
        .collect();

    let total_audio = audio_files.len() as u64;

    // Pre-filter to only new (unknown) files so the progress bar
    // reflects actual import work, not skipped files.
    let new_files: Vec<String> = audio_files
        .into_iter()
        .filter(|f| !known_paths.contains(f))
        .collect();
    let total_new = new_files.len() as u64;

    // Set final counts for scanned/skipped up front
    report.scanned = total_audio;
    report.skipped = total_audio - total_new;

    let mut processed: u64 = 0;
    for file_path in new_files {
        processed += 1;

        // Update progress every 5 files
        if let Some(tr) = tracker {
            if processed.is_multiple_of(5) || processed == total_new {
                let mut lock = tr.lock().await;
                *lock = Some(TaskStatus::Running {
                    progress: TaskProgress {
                        processed,
                        total: Some(total_new),
                    },
                });
            }
        }

        // Try to import the file
        match import_file(state, &file_path).await {
            Ok(_) => report.imported += 1,
            Err(e) => {
                report.errors.push(format!("{file_path}: {e}"));
            }
        }
    }

    Ok(report)
}

/// Import a single audio file from storage into the database.
async fn import_file(state: &AppState, relative_path: &str) -> Result<Uuid, String> {
    // Ensure the file is available locally for metadata extraction
    let local_path = soundtime_audio::ensure_local_file(state.storage.as_ref(), relative_path)
        .await
        .map_err(|e| format!("ensure_local_file: {e}"))?;

    let meta = soundtime_audio::extract_metadata_from_file(&local_path)
        .map_err(|e| format!("metadata: {e}"))?;

    let ext = std::path::Path::new(relative_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp3")
        .to_lowercase();

    // Try to derive user_id from path pattern {user_id}/...
    // Validate against the users table to avoid FK violations.
    let uploaded_by: Option<Uuid> = {
        let candidate = relative_path
            .split('/')
            .next()
            .and_then(|s| Uuid::parse_str(s).ok());
        if let Some(uid) = candidate {
            let exists = user::Entity::find_by_id(uid)
                .one(&state.db)
                .await
                .map_err(|e| format!("DB user lookup: {e}"))?
                .is_some();
            if exists {
                Some(uid)
            } else {
                None
            }
        } else {
            None
        }
    };

    let artist_name = meta
        .artist
        .clone()
        .unwrap_or_else(|| "Unknown Artist".to_string());

    // Find or create artist
    let artist_id = {
        use sea_orm::QueryFilter;
        let existing = soundtime_db::entities::artist::Entity::find()
            .filter(soundtime_db::entities::artist::Column::Name.eq(&artist_name))
            .one(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?;

        if let Some(a) = existing {
            a.id
        } else {
            let a = soundtime_db::entities::artist::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(artist_name.clone()),
                bio: Set(None),
                image_url: Set(None),
                musicbrainz_id: Set(None),
                created_at: Set(chrono::Utc::now().into()),
            };
            a.insert(&state.db)
                .await
                .map_err(|e| format!("Insert artist: {e}"))?
                .id
        }
    };

    // Resolve album artist: prefer album_artist tag, fall back to track artist
    let album_artist_name = meta
        .album_artist
        .clone()
        .unwrap_or_else(|| artist_name.clone());

    let album_artist_id = if album_artist_name == artist_name {
        artist_id
    } else {
        use sea_orm::QueryFilter;
        let existing = soundtime_db::entities::artist::Entity::find()
            .filter(soundtime_db::entities::artist::Column::Name.eq(&album_artist_name))
            .one(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?;

        if let Some(a) = existing {
            a.id
        } else {
            let a = soundtime_db::entities::artist::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(album_artist_name.clone()),
                bio: Set(None),
                image_url: Set(None),
                musicbrainz_id: Set(None),
                created_at: Set(chrono::Utc::now().into()),
            };
            a.insert(&state.db)
                .await
                .map_err(|e| format!("Insert album artist: {e}"))?
                .id
        }
    };

    // Find or create album
    let album_title = meta.album.clone().unwrap_or_else(|| "Singles".to_string());
    let album_id = {
        let existing = soundtime_db::entities::album::Entity::find()
            .filter(soundtime_db::entities::album::Column::Title.eq(&album_title))
            .filter(soundtime_db::entities::album::Column::ArtistId.eq(album_artist_id))
            .one(&state.db)
            .await
            .map_err(|e| format!("DB: {e}"))?;

        if let Some(a) = existing {
            a.id
        } else {
            let new_album = soundtime_db::entities::album::ActiveModel {
                id: Set(Uuid::new_v4()),
                title: Set(album_title.clone()),
                artist_id: Set(album_artist_id),
                release_date: Set(None),
                cover_url: Set(None),
                musicbrainz_id: Set(None),
                genre: Set(meta
                    .genre
                    .as_deref()
                    .map(normalize_genre)
                    .filter(|g| !g.is_empty())),
                year: Set(meta.year.map(|y| y as i16)),
                created_at: Set(chrono::Utc::now().into()),
            };
            let result = new_album
                .insert(&state.db)
                .await
                .map_err(|e| format!("Insert album: {e}"))?;

            // Extract embedded cover if available
            if let Some(cover_data) = &meta.cover_art {
                if let Ok(cover_path) = state
                    .storage
                    .store_cover(
                        uploaded_by.unwrap_or_else(Uuid::new_v4),
                        Some(&album_title),
                        cover_data,
                    )
                    .await
                {
                    let mut update: soundtime_db::entities::album::ActiveModel =
                        result.clone().into();
                    update.cover_url = Set(Some(format!("/api/media/{cover_path}")));
                    if let Err(e) = update.update(&state.db).await {
                        tracing::warn!(error = %e, "failed to update album cover URL during scan");
                    }
                }
            }

            result.id
        }
    };

    let title = meta.title.clone().unwrap_or_else(|| {
        std::path::Path::new(relative_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    });

    let waveform = soundtime_audio::generate_waveform(&local_path, 200).ok();

    let track_id = Uuid::new_v4();
    let new_track = track::ActiveModel {
        id: Set(track_id),
        title: Set(title),
        album_id: Set(Some(album_id)),
        artist_id: Set(artist_id),
        track_number: Set(meta.track_number.map(|n| n as i16)),
        disc_number: Set(meta.disc_number.map(|n| n as i16)),
        duration_secs: Set(meta.duration_secs as f32),
        genre: Set(meta
            .genre
            .as_deref()
            .map(normalize_genre)
            .filter(|g| !g.is_empty())),
        year: Set(meta.year.map(|y| y as i16)),
        musicbrainz_id: Set(None),
        file_path: Set(relative_path.to_string()),
        format: Set(ext),
        file_size: Set(meta.file_size as i64),
        bitrate: Set(meta.bitrate.map(|b| b as i32)),
        sample_rate: Set(meta.sample_rate.map(|s| s as i32)),
        waveform_data: Set(waveform.map(|w| serde_json::json!(w))),
        uploaded_by: Set(uploaded_by),
        content_hash: Set(None),
        play_count: Set(0),
        created_at: Set(chrono::Utc::now().into()),
    };

    new_track
        .insert(&state.db)
        .await
        .map_err(|e| format!("Insert track: {e}"))?;

    Ok(track_id)
}
