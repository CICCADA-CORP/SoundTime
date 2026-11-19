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
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait,
    FromQueryResult, PaginatorTrait, QueryFilter, QuerySelect, Set,
};
use serde::Serialize;
use soundtime_audio::metadata::normalize_genre;
use soundtime_db::entities::{track, user};
use soundtime_db::AppState;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Interval between automatic runs (24 hours).
const DAILY_INTERVAL_SECS: u64 = 86_400;

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
    pub moved: u64,
    pub removed: u64,
    pub empty_sources: Vec<String>,
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

/// Reserve the tracker before spawning work. Daily and manual runs share it.
pub async fn try_start(tracker: &TaskTrackerHandle) -> bool {
    let mut slot = tracker.lock().await;
    if matches!(*slot, Some(TaskStatus::Running { .. })) {
        return false;
    }
    *slot = Some(TaskStatus::Running {
        progress: TaskProgress {
            processed: 0,
            total: None,
        },
    });
    true
}

pub fn spawn(state: Arc<AppState>, tracker: TaskTrackerHandle) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(DAILY_INTERVAL_SECS)).await;
            if !try_start(&tracker).await {
                continue;
            }
            match run_integrity_check(&state, Some(&tracker)).await {
                Ok(report) => tracing::info!(
                    checked = report.total_checked,
                    healthy = report.healthy,
                    missing = report.missing.len(),
                    "daily storage integrity check finished"
                ),
                Err(e) => tracing::error!("daily storage integrity check failed: {e}"),
            }
            let result = match run_sync(&state, Some(&tracker), &[]).await {
                Ok(report) => {
                    tracing::info!(
                        scanned = report.scanned,
                        imported = report.imported,
                        moved = report.moved,
                        removed = report.removed,
                        errors = report.errors.len(),
                        "daily storage sync finished"
                    );
                    TaskStatus::Completed {
                        result: TaskResult::Sync(report),
                    }
                }
                Err(message) => {
                    tracing::error!("daily storage sync failed: {message}");
                    TaskStatus::Error { message }
                }
            };
            *tracker.lock().await = Some(result);
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
        .filter(crate::storage_catalog::reusable_track())
        .filter(track::Column::FilePath.not_like("p2p://%"))
        .paginate(&state.db, 500);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| format!("DB count: {e}"))?;

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

            match state.storage.file_exists(&t.file_path).await {
                Ok(true) => {}
                Ok(false) => {
                    report.missing.push(MissingTrack {
                        track_id: t.id.to_string(),
                        title: t.title.clone(),
                        file_path: t.file_path.clone(),
                    });
                    continue;
                }
                Err(e) => {
                    report.errors.push(format!("{}: {e}", t.file_path));
                    continue;
                }
            }

            // Verify file is readable (hash check)
            match state.storage.hash_file(&t.file_path).await {
                Ok(actual)
                    if t.file_hash
                        .as_ref()
                        .is_some_and(|expected| *expected != actual) =>
                {
                    report.errors.push(format!(
                        "Track {} ({}): SHA-256 differs from the catalog",
                        t.id, t.title
                    ));
                }
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

#[derive(FromQueryResult)]
struct CatalogEntry {
    id: Uuid,
    file_path: String,
    file_size: i64,
    file_hash: Option<String>,
    content_hash: Option<String>,
}

/// A returned mount may now contain a different file at the same path. Read one
/// version and backfill SHA-256 only if an existing fingerprint attests its bytes.
async fn restored_sha256(
    storage: &dyn soundtime_audio::StorageBackend,
    row: &CatalogEntry,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    if row.file_hash.is_none() && row.content_hash.is_none() {
        return Err("missing historical fingerprint; original cannot be identified".into());
    }
    let file = storage
        .open_media(&row.file_path)
        .await
        .map_err(|e| e.to_string())?;
    let size = file.size();
    if i64::try_from(size).ok() != Some(row.file_size) {
        return Err("contents differ from the missing original".into());
    }
    let mut sha = Sha256::new();
    let mut blob = row.file_hash.is_none().then(soundtime_p2p::BlobHasher::new);
    if size > 0 {
        let mut stream = file.range(0, size).await.map_err(|e| e.to_string())?;
        while let Some(bytes) = stream.next_chunk().await.map_err(|e| e.to_string())? {
            sha.update(&bytes);
            if let Some(blob) = &mut blob {
                blob.update(&bytes);
            }
        }
    }
    let sha = format!("{:x}", sha.finalize());
    let valid = match (row.file_hash.as_ref(), blob, row.content_hash.as_ref()) {
        (Some(expected), _, _) => expected.eq_ignore_ascii_case(&sha),
        (None, Some(blob), Some(expected)) => {
            expected.eq_ignore_ascii_case(blob.finalize().to_hex().as_str())
        }
        _ => false,
    };
    if valid {
        Ok(sha)
    } else {
        Err("contents differ from the missing original".into())
    }
}

// Compose mounts the optional NAS volumes independently underneath the local
// music volume. A working local library does not prove that either share is up.
fn media_source(path: &str) -> &'static str {
    match path.split('/').next() {
        Some("network-nfs") => "network-nfs",
        Some("network-smb") => "network-smb",
        _ => "/",
    }
}

pub async fn run_sync(
    state: &AppState,
    tracker: Option<&TaskTrackerHandle>,
    allow_empty: &[&str],
) -> Result<SyncReport, String> {
    // Snapshot the catalog first. Rows created after this point cannot be pruned.
    let entries = track::Entity::find()
        .filter(crate::storage_catalog::reusable_track())
        .filter(track::Column::FilePath.not_like("p2p://%"))
        .select_only()
        .columns([
            track::Column::Id,
            track::Column::FilePath,
            track::Column::FileSize,
            track::Column::FileHash,
            track::Column::ContentHash,
        ])
        .into_model::<CatalogEntry>()
        .all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // A partial listing (for example an inaccessible subdirectory) is not evidence of deletion.
    let listed: HashSet<String> = state
        .storage
        .list_files("")
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let known: HashSet<&str> = entries.iter().map(|r| r.file_path.as_str()).collect();
    let mut new_files = Vec::new();
    let mut populated_sources = HashSet::new();
    let mut report = SyncReport {
        scanned: 0,
        imported: 0,
        skipped: 0,
        moved: 0,
        removed: 0,
        errors: vec![],
        empty_sources: Vec::new(),
    };
    for file in &listed {
        if Path::new(file)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(soundtime_audio::metadata::is_supported_format)
        {
            populated_sources.insert(media_source(file));
            report.scanned += 1;
            if known.contains(file.as_str()) {
                report.skipped += 1;
            } else {
                new_files.push(file);
            }
        }
    }
    new_files.sort_unstable();

    let absent_ids: HashSet<Uuid> = state
        .db
        .query_all(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Postgres,
            "SELECT track_id FROM federation_missing_sources",
        ))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.try_get("", "track_id"))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut missing = Vec::new();
    let mut present_by_size: HashMap<i64, Vec<Uuid>> = HashMap::new();
    let mut legacy_by_size: HashMap<i64, Vec<Uuid>> = HashMap::new();
    for row in &entries {
        if !listed.contains(&row.file_path) {
            missing.push(row);
            if row.file_hash.is_none() {
                legacy_by_size
                    .entry(row.file_size)
                    .or_default()
                    .push(row.id);
            }
        } else {
            if absent_ids.contains(&row.id) {
                let sha = match restored_sha256(state.storage.as_ref(), row).await {
                    Ok(sha) => sha,
                    Err(err) => {
                        report.errors.push(format!("{}: {err}", row.file_path));
                        continue;
                    }
                };
                let tx = crate::storage_catalog::begin_write(&state.db)
                    .await
                    .map_err(|e| e.to_string())?;
                let confirmed=tx.execute(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
                    "UPDATE tracks t SET file_hash=$3 WHERE id=$1 AND file_path=$2 AND file_hash IS NOT DISTINCT FROM $4
                     AND content_hash IS NOT DISTINCT FROM $5 AND NOT EXISTS(
                         SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn')",
                    [row.id.into(),row.file_path.as_str().into(),sha.into(),row.file_hash.clone().into(),
                        row.content_hash.clone().into()])).await.map_err(|e|e.to_string())?;
                if confirmed.rows_affected() == 1 {
                    crate::storage_catalog::source_restored(&tx, row.id)
                        .await
                        .map_err(|e| e.to_string())?;
                    present_by_size
                        .entry(row.file_size)
                        .or_default()
                        .push(row.id);
                }
                tx.commit().await.map_err(|e| e.to_string())?;
                continue;
            }
            present_by_size
                .entry(row.file_size)
                .or_default()
                .push(row.id);
            if row.file_hash.is_some() {
                continue;
            }
            // Index existing files before checking new ones for moves/copies.
            match state.storage.hash_file(&row.file_path).await {
                Ok(hash) => {
                    track::Entity::update_many()
                        .col_expr(
                            track::Column::FileHash,
                            sea_orm::sea_query::Expr::value(hash),
                        )
                        .filter(track::Column::Id.eq(row.id))
                        .filter(track::Column::FilePath.eq(&row.file_path))
                        .filter(track::Column::FileHash.is_null())
                        .filter(crate::storage_catalog::reusable_track())
                        .exec(&state.db)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Err(e) => report.errors.push(format!("{}: {e}", row.file_path)),
            }
        }
    }

    // Include withdrawn historical originals. A move may race the background SHA-256 job.
    let legacy_blob_sizes: HashSet<i64> = if new_files.is_empty() {
        HashSet::new()
    } else {
        state.db.query_all(sea_orm::Statement::from_string(sea_orm::DbBackend::Postgres,
            "SELECT DISTINCT file_size FROM tracks WHERE file_hash IS NULL AND content_hash IS NOT NULL
             AND file_path<>'' AND file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'"))
            .await.map_err(|e| e.to_string())?.into_iter().map(|r| r.try_get("", "file_size"))
            .collect::<Result<_,_>>().map_err(|e| e.to_string())?
    };
    let total = new_files.len() as u64;
    for (idx, path) in new_files.into_iter().enumerate() {
        match sync_file(state, path, &legacy_by_size, &legacy_blob_sizes).await {
            Ok(SyncOutcome::Imported) => report.imported += 1,
            Ok(SyncOutcome::Moved) => report.moved += 1,
            Ok(SyncOutcome::Present) => report.skipped += 1,
            Err(e) => report.errors.push(format!("{path}: {e}")),
        }
        let processed = idx as u64 + 1;
        if let Some(tr) = tracker {
            if processed.is_multiple_of(5) || processed == total {
                *tr.lock().await = Some(TaskStatus::Running {
                    progress: TaskProgress {
                        processed,
                        total: Some(total),
                    },
                });
            }
        }
    }

    // A failed import may still be a relocated track. Keep its history for a retry.
    if !missing.is_empty() && report.errors.is_empty() {
        // An inaccessible path rolls back the entire cleanup. Holding the same lock
        // as uploads also prevents removing a row while its file is being restored.
        let tx = crate::storage_catalog::begin_write(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        for row in missing {
            match state.storage.file_exists(&row.file_path).await {
                Ok(true) => continue,
                Err(e) => return Err(format!("{}: {e}", row.file_path)),
                Ok(false) => {}
            }
            let source = media_source(&row.file_path);
            if !populated_sources.contains(source) && !allow_empty.contains(&source) {
                let still_missing = track::Entity::find_by_id(row.id)
                    .filter(track::Column::FilePath.eq(&row.file_path))
                    .count(&tx)
                    .await
                    .map_err(|e| e.to_string())?;
                if still_missing > 0 && !report.empty_sources.iter().any(|s| s == source) {
                    report.empty_sources.push(source.to_string());
                }
            }
            // A second historical row may own these bytes. Keep both UUIDs and relink only a verified match.
            if let Some(ids) = present_by_size.get(&row.file_size) {
                if repair_previous_import(&tx, state, row, ids).await? {
                    report.moved += 1;
                    continue;
                }
            }
            // Moves and concurrent changes win over an earlier snapshot.
            let result = tx.execute(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
                "INSERT INTO federation_missing_sources(track_id) SELECT t.id FROM tracks t \
                 WHERE t.id=$1 AND t.file_path=$2 AND NOT EXISTS(SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn') \
                 ON CONFLICT DO NOTHING",
                [row.id.into(), row.file_path.as_str().into()])).await.map_err(|e|e.to_string())?;
            report.removed += result.rows_affected();
        }
        tx.commit().await.map_err(|e| e.to_string())?;
    }
    report.empty_sources.sort();
    Ok(report)
}

async fn repair_previous_import(
    tx: &DatabaseTransaction,
    state: &AppState,
    entry: &CatalogEntry,
    present_ids: &[Uuid],
) -> Result<bool, String> {
    let Some(missing) = track::Entity::find_by_id(entry.id)
        .filter(crate::storage_catalog::reusable_track())
        .filter(track::Column::FilePath.eq(&entry.file_path))
        .lock_exclusive()
        .one(tx)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let candidates = track::Entity::find()
        .filter(crate::storage_catalog::reusable_track())
        .filter(track::Column::Id.is_in(present_ids.iter().copied()))
        .lock_exclusive()
        .all(tx)
        .await
        .map_err(|e| e.to_string())?;
    let mut matches = candidates.into_iter().filter(|r| match &missing.file_hash {
        Some(hash) => r.file_hash.as_ref() == Some(hash),
        None => match (&missing.content_hash, &r.content_hash) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
    });
    let Some(present) = matches.next() else {
        return Ok(false);
    };
    if matches.next().is_some()
        || !state
            .storage
            .file_exists(&present.file_path)
            .await
            .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    let actual = state
        .storage
        .hash_file(&present.file_path)
        .await
        .map_err(|e| e.to_string())?;
    if present.file_hash.as_deref() != Some(actual.as_str()) {
        return Ok(false);
    }
    // Distinct publications may share a file. Relinking never transfers playlists,
    // listens or ownership between their stable IDs.
    track::Entity::update_many()
        .col_expr(
            track::Column::FilePath,
            sea_orm::sea_query::Expr::value(present.file_path),
        )
        .col_expr(
            track::Column::FileHash,
            sea_orm::sea_query::Expr::value(actual),
        )
        .filter(track::Column::Id.eq(missing.id))
        .exec(tx)
        .await
        .map_err(|e| e.to_string())?;
    crate::storage_catalog::source_restored(tx, missing.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

enum SyncOutcome {
    Present,
    Moved,
    Imported,
}

async fn legacy_blob_hash(
    path: &Path,
    expected_sha: &str,
    sizes: &HashSet<i64>,
) -> Result<Option<String>, String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    if sizes.is_empty() {
        return Ok(None);
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let size = file.metadata().await.map_err(|e| e.to_string())?.len();
    if !i64::try_from(size).is_ok_and(|size| sizes.contains(&size)) {
        return Ok(None);
    }
    let mut blob = soundtime_p2p::BlobHasher::new();
    let mut sha = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        blob.update(&buffer[..read]);
        sha.update(&buffer[..read]);
    }
    if format!("{:x}", sha.finalize()) != expected_sha {
        return Err("original changed while it was being scanned".into());
    }
    Ok(Some(blob.finalize().to_hex().to_string()))
}

async fn sync_file(
    state: &AppState,
    path: &str,
    legacy_by_size: &HashMap<i64, Vec<Uuid>>,
    legacy_blob_sizes: &HashSet<i64>,
) -> Result<SyncOutcome, String> {
    let source = state
        .storage
        .prepare_analysis(path)
        .await
        .map_err(|e| e.to_string())?;
    let hash = source.sha256.clone();
    let blob_hash = legacy_blob_hash(&source.path, &hash, legacy_blob_sizes).await?;
    let tx = crate::storage_catalog::begin_write(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    // Uploads and other processes can add this file while the scan is in progress.
    if track::Entity::find()
        .filter(track::Column::FilePath.eq(path))
        .count(&tx)
        .await
        .map_err(|e| e.to_string())?
        > 0
    {
        return Ok(SyncOutcome::Present);
    }
    if let Some(row) =
        crate::storage_catalog::existing_hash(&tx, &hash, state.storage.as_ref(), None).await?
    {
        if state
            .storage
            .file_exists(&row.file_path)
            .await
            .map_err(|e| e.to_string())?
        {
            tx.commit().await.map_err(|e| e.to_string())?;
            return Ok(SyncOutcome::Present);
        }
        return relink_track(tx, row, path, hash).await;
    }
    let excluded: bool = tx.query_one(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
        "SELECT EXISTS(SELECT FROM tracks t JOIN track_publications p ON p.track_id=t.id WHERE p.state='withdrawn' \
         AND (t.file_path=$1 OR t.file_hash=$2 OR (t.file_hash IS NULL AND t.content_hash=$3))) AS excluded", [path.into(), hash.as_str().into(), blob_hash.clone().into()]))
        .await.map_err(|e|e.to_string())?.expect("EXISTS returns a row").try_get("", "excluded").map_err(|e|e.to_string())?;
    if excluded {
        return Ok(SyncOutcome::Present);
    }
    let local_path = &source.path;
    let meta = soundtime_audio::extract_metadata_from_file(local_path)
        .map_err(|e| format!("metadata: {e}"))?;
    let waveform = soundtime_audio::generate_waveform(local_path, 200)
        .ok()
        .map(|w| serde_json::json!(w));

    // Pre-upgrade rows may already be missing before their first SHA-256 backfill.
    // Recover a unique match using the P2P blob hash, or the stored audio waveform,
    // file size, duration and filename/tagged title. A title alone is ambiguous.
    if let Some(ids) = legacy_by_size.get(&(meta.file_size as i64)) {
        let candidates = track::Entity::find()
            .filter(crate::storage_catalog::reusable_track())
            .filter(track::Column::Id.is_in(ids.clone()))
            .filter(track::Column::FileHash.is_null())
            .all(&tx)
            .await
            .map_err(|e| e.to_string())?;
        let mut matches = candidates.into_iter().filter(|r| match &r.content_hash {
            Some(h) => blob_hash.as_ref() == Some(h),
            None => {
                waveform.is_some()
                    && r.waveform_data == waveform
                    && r.duration_secs == meta.duration_secs as f32
                    && (Path::new(&r.file_path).file_name() == Path::new(path).file_name()
                        || meta.title.as_deref() == Some(r.title.as_str()))
            }
        });
        if let Some(row) = matches.next() {
            if matches.next().is_none()
                && !state
                    .storage
                    .file_exists(&row.file_path)
                    .await
                    .map_err(|e| e.to_string())?
            {
                return relink_track(tx, row, path, hash).await;
            }
        }
    }
    import_file(state, &tx, path, hash, meta, waveform).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(SyncOutcome::Imported)
}

async fn relink_track(
    tx: DatabaseTransaction,
    row: track::Model,
    path: &str,
    hash: String,
) -> Result<SyncOutcome, String> {
    crate::storage_catalog::source_restored(&tx, row.id)
        .await
        .map_err(|e| e.to_string())?;
    let mut update: track::ActiveModel = row.into();
    update.file_path = Set(path.to_string());
    update.file_hash = Set(Some(hash));
    update.update(&tx).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(SyncOutcome::Moved)
}

async fn import_file(
    state: &AppState,
    tx: &DatabaseTransaction,
    relative_path: &str,
    hash: String,
    meta: soundtime_audio::AudioMetadata,
    waveform: Option<serde_json::Value>,
) -> Result<Uuid, String> {
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
                .one(tx)
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
            .one(tx)
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
            a.insert(tx)
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
            .one(tx)
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
            a.insert(tx)
                .await
                .map_err(|e| format!("Insert album artist: {e}"))?
                .id
        }
    };

    // Find or create album
    let album_title = meta.album.clone().unwrap_or_else(|| "Singles".to_string());
    let album = {
        let existing = soundtime_db::entities::album::Entity::find()
            .filter(soundtime_db::entities::album::Column::Title.eq(&album_title))
            .filter(soundtime_db::entities::album::Column::ArtistId.eq(album_artist_id))
            .one(tx)
            .await
            .map_err(|e| format!("DB: {e}"))?;

        if let Some(a) = existing {
            a
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
            new_album
                .insert(tx)
                .await
                .map_err(|e| format!("Insert album: {e}"))?
        }
    };
    let album_id = album.id;
    if let (None, Some(cover_data)) = (&album.cover_url, &meta.cover_art) {
        // The first track scanned for an album may have no embedded artwork.
        match state
            .storage
            .store_cover(
                uploaded_by.unwrap_or(album_id),
                Some(&album_title),
                cover_data,
            )
            .await
        {
            Ok(cover_path) => {
                let mut update: soundtime_db::entities::album::ActiveModel = album.into();
                update.cover_url = Set(Some(format!("/api/media/{cover_path}")));
                if let Err(e) = update.update(tx).await {
                    tracing::warn!(%album_id, error = %e, "failed to update album cover URL during scan");
                }
            }
            Err(e) => {
                tracing::warn!(%album_id, error = %e, "failed to store album cover during scan")
            }
        }
    }

    let title = meta.title.clone().unwrap_or_else(|| {
        std::path::Path::new(relative_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    });

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
        waveform_data: Set(waveform),
        uploaded_by: Set(uploaded_by),
        content_hash: Set(None),
        file_hash: Set(Some(hash)),
        play_count: Set(0),
        created_at: Set(chrono::Utc::now().into()),
    };

    new_track
        .insert(tx)
        .await
        .map_err(|e| format!("Insert track: {e}"))?;

    Ok(track_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Schema};
    use soundtime_db::entities::{album, artist, remote_track};

    #[tokio::test]
    async fn only_one_scan_can_reserve_the_tracker() {
        let tracker = new_tracker();
        let (a, b) = tokio::join!(try_start(&tracker), try_start(&tracker));
        assert_ne!(a, b);
    }

    async fn share_snapshot(db: &sea_orm::DatabaseConnection, id: Uuid) -> (bool, String) {
        let row = db.query_one(sea_orm::Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=t.id) AS missing,
                jsonb_build_array(t.id,p.state,t.file_path,t.file_hash,
                    (SELECT jsonb_agg(jsonb_build_array(playlist_id,position) ORDER BY playlist_id)
                     FROM playlist_tracks WHERE track_id=t.id),
                    (SELECT jsonb_agg(user_id ORDER BY user_id) FROM favorites WHERE track_id=t.id),
                    (SELECT count(*) FROM listen_history WHERE track_id=t.id))::text AS refs
             FROM tracks t JOIN track_publications p ON p.track_id=t.id WHERE t.id=$1",
            [id.into()])).await.unwrap().unwrap();
        (
            row.try_get("", "missing").unwrap(),
            row.try_get("", "refs").unwrap(),
        )
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL for a PostgreSQL connection"]
    async fn reconcile_storage_with_postgres() {
        use std::fs;
        crate::storage_catalog::tests::assert_postgres_write_lock().await;
        let mut opts = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
        // Use connection-local temporary tables, including when testing against a shared DB.
        opts.max_connections(1).min_connections(1);
        let db = Database::connect(opts).await.unwrap();
        db.execute_unprepared("CREATE TEMP TABLE users (id uuid PRIMARY KEY)")
            .await
            .unwrap();
        let backend = DbBackend::Postgres;
        db.execute_unprepared(
            "CREATE TEMP TABLE track_publications(track_id uuid PRIMARY KEY, state text NOT NULL);
            CREATE TEMP TABLE federation_missing_sources(track_id uuid PRIMARY KEY);
            CREATE TEMP TABLE federation_managed_originals(file_path text PRIMARY KEY,sha256 text NOT NULL)",
        )
        .await
        .unwrap();
        let schema = Schema::new(backend);
        for mut table in [
            schema.create_table_from_entity(artist::Entity),
            schema.create_table_from_entity(album::Entity),
            schema.create_table_from_entity(track::Entity),
            schema.create_table_from_entity(remote_track::Entity),
        ] {
            db.execute(backend.build(table.temporary())).await.unwrap();
        }
        for (table, key) in [
            ("library_tracks", "library_id"),
            ("playlist_tracks", "playlist_id"),
            ("favorites", "user_id"),
        ] {
            db.execute_unprepared(&format!("CREATE TEMP TABLE {table} ({key} uuid, track_id uuid REFERENCES tracks ON DELETE CASCADE, PRIMARY KEY({key},track_id))")).await.unwrap();
        }
        for table in ["listen_history", "track_report", "track_embeddings"] {
            db.execute_unprepared(&format!(
                "CREATE TEMP TABLE {table} (track_id uuid REFERENCES tracks ON DELETE CASCADE)"
            ))
            .await
            .unwrap();
        }
        db.execute_unprepared(
            "ALTER TABLE playlist_tracks ADD COLUMN position integer NOT NULL DEFAULT 0",
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState {
            db,
            jwt_secret: String::new(),
            domain: "localhost".into(),
            storage: Arc::new(soundtime_audio::AudioStorage::new(dir.path())),
            p2p: None,
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
        });
        // Valid 8-bit mono PCM, 256 samples at 8 kHz.
        let mut wav = b"RIFF\x24\x01\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x40\x1f\x00\x00\x40\x1f\x00\x00\x01\x00\x08\x00data\x00\x01\x00\x00".to_vec();
        wav.extend(0..=255);
        fs::write(dir.path().join("one.wav"), &wav).unwrap();
        let first = run_sync(&state, None, &[]).await.unwrap();
        assert_eq!(first.imported, 1, "{:?}", first.errors);
        let first = track::Entity::find().one(&state.db).await.unwrap().unwrap();

        // Postgres SUM(bigint) returns numeric. Exercise the storage status route with actual decoding.
        let stats = crate::api::admin::storage_status(axum::extract::State(state.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(stats.total_size_bytes, wav.len() as i64);

        let mut legacy: track::ActiveModel = first.clone().into();
        legacy.file_hash = Set(None);
        legacy.play_count = Set(9);
        legacy.update(&state.db).await.unwrap();
        fs::create_dir(dir.path().join("moved")).unwrap();
        fs::rename(dir.path().join("one.wav"), dir.path().join("moved/one.wav")).unwrap();
        let matched = run_sync(&state, None, &[]).await.unwrap();
        assert_eq!(
            (matched.moved, matched.imported, matched.removed),
            (1, 0, 0),
            "{:?}",
            matched.errors
        );
        let track = track::Entity::find_by_id(first.id)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(track.file_path, "moved/one.wav");
        assert_eq!(track.play_count, 9);
        assert!(track.file_hash.is_some());

        fs::copy(
            dir.path().join("moved/one.wav"),
            dir.path().join("copy.wav"),
        )
        .unwrap();
        let copies = run_sync(&state, None, &[]).await.unwrap();
        assert_eq!((copies.skipped, copies.imported), (2, 0));
        fs::remove_file(dir.path().join("moved/one.wav")).unwrap();
        let moved = run_sync(&state, None, &[]).await.unwrap();
        assert_eq!((moved.moved, moved.removed), (1, 0));

        // Two historical publications retain their personal references while sharing a verified file.
        fs::rename(
            dir.path().join("copy.wav"),
            dir.path().join("duplicate.wav"),
        )
        .unwrap();
        let original_id = first.id;
        let duplicate_id = Uuid::new_v4();
        let mut duplicate: track::ActiveModel = first.clone().into();
        duplicate.id = Set(duplicate_id);
        duplicate.file_path = Set("duplicate.wav".into());
        duplicate.created_at = Set(chrono::Utc::now().into());
        duplicate.file_hash = Set(None);
        duplicate.play_count = Set(3);
        duplicate.insert(&state.db).await.unwrap();
        let listener = Uuid::new_v4();
        state
            .db
            .execute(sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO favorites VALUES ($1,$2)",
                [listener.into(), duplicate_id.into()],
            ))
            .await
            .unwrap();
        state
            .db
            .execute(sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO listen_history VALUES ($1)",
                [duplicate_id.into()],
            ))
            .await
            .unwrap();
        let repaired = run_sync(&state, None, &[]).await.unwrap();
        assert_eq!((repaired.moved, repaired.removed), (1, 0));
        let kept = track::Entity::find_by_id(original_id)
            .one(&state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.file_path, "duplicate.wav");
        assert_eq!(kept.play_count, 9);
        for table in ["favorites", "listen_history"] {
            let row = state
                .db
                .query_one(sea_orm::Statement::from_string(
                    backend,
                    format!("SELECT track_id FROM {table}"),
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.try_get::<Uuid>("", "track_id").unwrap(), duplicate_id);
        }
        // An offline share must survive a scan even if local music is present.
        let share = dir.path().join("network-nfs");
        fs::create_dir(&share).unwrap();
        let mut other_audio = wav.clone();
        *other_audio.last_mut().unwrap() = 0;
        fs::write(share.join("nas.wav"), &other_audio).unwrap();
        assert_eq!(run_sync(&state, None, &[]).await.unwrap().imported, 1);
        let nas = track::Entity::find()
            .filter(track::Column::FilePath.eq("network-nfs/nas.wav"))
            .one(&state.db)
            .await
            .unwrap()
            .unwrap();
        let nas_id = nas.id;
        for query in [
            "INSERT INTO track_publications VALUES($1,'active')",
            "INSERT INTO playlist_tracks VALUES(gen_random_uuid(),$1,2),(gen_random_uuid(),$1,7)",
            "INSERT INTO favorites VALUES(gen_random_uuid(),$1)",
            "INSERT INTO listen_history VALUES($1),($1)",
        ] {
            state
                .db
                .execute(sea_orm::Statement::from_sql_and_values(
                    backend,
                    query,
                    [nas_id.into()],
                ))
                .await
                .unwrap();
        }
        let (missing, refs) = share_snapshot(&state.db, nas_id).await;
        assert!(!missing);
        fs::remove_dir_all(&share).unwrap();
        let guarded = run_sync(&state, None, &["network-smb"]).await.unwrap();
        assert_eq!(guarded.removed, 1);
        assert_eq!(guarded.empty_sources, ["network-nfs"]);
        assert_eq!(
            share_snapshot(&state.db, nas_id).await,
            (true, refs.clone())
        );
        assert_eq!(
            run_sync(&state, None, &["network-nfs"])
                .await
                .unwrap()
                .removed,
            0
        );
        fs::create_dir(&share).unwrap();
        // A replacement of the same length cannot silently take over the old playlist entries.
        fs::write(share.join("nas.wav"), &wav).unwrap();
        let different = run_sync(&state, None, &[]).await.unwrap();
        assert!(different
            .errors
            .iter()
            .any(|error| error.contains("contents differ")));
        assert_eq!(
            share_snapshot(&state.db, nas_id).await,
            (true, refs.clone())
        );
        fs::write(share.join("nas.wav"), &other_audio).unwrap();
        let restored = run_sync(&state, None, &[]).await.unwrap();
        assert!(restored.errors.is_empty(), "{:?}", restored.errors);
        assert_eq!(
            (restored.imported, restored.moved, restored.removed),
            (0, 0, 0)
        );
        assert_eq!(
            share_snapshot(&state.db, nas_id).await,
            (false, refs.clone())
        );
        let source = state.storage.open_media(&nas.file_path).await.unwrap();
        let mut tail = source.range(other_audio.len() as u64 - 4, 4).await.unwrap();
        assert_eq!(
            tail.next_chunk().await.unwrap().unwrap(),
            other_audio[other_audio.len() - 4..]
        );
        fs::remove_dir_all(&share).unwrap();
        assert_eq!(run_sync(&state, None, &[]).await.unwrap().removed, 1);
        assert_eq!(share_snapshot(&state.db, nas_id).await, (true, refs));
        state
            .db
            .execute(sea_orm::Statement::from_sql_and_values(
                backend,
                "UPDATE tracks SET file_hash=NULL,content_hash=NULL WHERE id=$1",
                [nas_id.into()],
            ))
            .await
            .unwrap();
        fs::create_dir(&share).unwrap();
        fs::write(share.join("nas.wav"), &other_audio).unwrap();
        let unverified = run_sync(&state, None, &[]).await.unwrap();
        assert!(unverified
            .errors
            .iter()
            .any(|e| e.contains("missing historical fingerprint")));
        assert!(share_snapshot(&state.db, nas_id).await.0);
        let blob = soundtime_p2p::BlobHash::new(&other_audio).to_string();
        state
            .db
            .execute(sea_orm::Statement::from_sql_and_values(
                backend,
                "UPDATE tracks SET content_hash=$2 WHERE id=$1",
                [nas_id.into(), blob.into()],
            ))
            .await
            .unwrap();
        assert!(run_sync(&state, None, &[]).await.unwrap().errors.is_empty());
        assert!(!share_snapshot(&state.db, nas_id).await.0);
        assert_eq!(
            track::Entity::find_by_id(nas_id)
                .one(&state.db)
                .await
                .unwrap()
                .unwrap()
                .file_hash,
            nas.file_hash
        );
        fs::remove_dir_all(&share).unwrap();
        assert_eq!(run_sync(&state, None, &[]).await.unwrap().removed, 1);
        let mut remote: track::ActiveModel = first.into();
        remote.id = Set(Uuid::new_v4());
        remote.file_path = Set("p2p://node/hash".into());
        remote.insert(&state.db).await.unwrap();
        let stats = crate::api::admin::storage_status(axum::extract::State(state.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(stats.total_tracks, 2);
        assert_eq!(stats.total_size_bytes, wav.len() as i64);
        fs::remove_file(dir.path().join("duplicate.wav")).unwrap();
        fs::write(dir.path().join("unreadable.mp3"), b"bad audio").unwrap();
        let failed = run_sync(&state, None, &[]).await.unwrap();
        assert!(!failed.errors.is_empty());
        assert_eq!(failed.removed, 0);
        fs::remove_file(dir.path().join("unreadable.mp3")).unwrap();
        assert!(run_sync(&state, None, &[])
            .await
            .unwrap()
            .empty_sources
            .contains(&"/".to_string()));
        let cleaned = run_sync(&state, None, &["/"]).await.unwrap();
        assert_eq!(cleaned.removed, 0);
        assert_eq!(track::Entity::find().count(&state.db).await.unwrap(), 4);
        assert_eq!(run_sync(&state, None, &["/"]).await.unwrap().removed, 0);
        let user_id = Uuid::new_v4();
        state
            .db
            .execute(sea_orm::Statement::from_sql_and_values(
                backend,
                "INSERT INTO users (id) VALUES ($1)",
                [user_id.into()],
            ))
            .await
            .unwrap();
        crate::api::audio::check_duplicate_uploads(&state, user_id, wav).await;
    }
}
