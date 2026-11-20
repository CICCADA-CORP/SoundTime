//! Publication authority and cleanup of original files owned by this instance.
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, Statement};
use sha2::{Digest, Sha256};
use soundtime_audio::storage::{MediaVersion, StorageError};
use soundtime_db::entities::track;
use soundtime_db::AppState;
use std::sync::Arc;
use uuid::Uuid;

pub struct Change {
    id: Uuid,
    hashes: Vec<String>,
    local: bool,
    changed: bool,
}

impl Change {
    /// Only call after the transaction containing the withdrawal commits.
    pub async fn committed(self, state: &AppState) {
        soundtime_db::media_access::revoke_track(self.id);
        for hash in self.hashes {
            soundtime_db::media_access::revoke_legacy_hash(&hash);
        }
        if self.changed {
            if let Some(node) = state
                .p2p
                .as_ref()
                .and_then(|p| p.clone().downcast::<soundtime_p2p::P2pNode>().ok())
            {
                node.search_index().mark_dirty().await;
                if self.local {
                    node.announce_withdrawal(self.id).await;
                }
            }
            if self.local {
                if let Some(registry) = crate::api::get_plugin_registry(state) {
                    let payload = serde_json::json!({"track_id":self.id});
                    tokio::spawn(async move {
                        registry.dispatch("on_track_deleted", &payload).await;
                    });
                }
            }
        }
    }
}

pub async fn local(db: &impl ConnectionTrait, id: Uuid) -> Result<bool, DbErr> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT CASE WHEN p.track_id IS NULL THEN t.file_path <> '' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:' \
         ELSE coalesce(c.is_local,false) END AS local FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id \
         LEFT JOIN federation_catalogs c ON c.id=p.catalog_id WHERE t.id=$1", [id.into()])).await?
        .ok_or_else(|| DbErr::RecordNotFound("track".into()))?.try_get("", "local")
}

/// Caller holds the catalogue writer lock and has verified the actor's role or uploader ID.
pub async fn remove(
    tx: &DatabaseTransaction,
    trk: &track::Model,
    local: bool,
) -> Result<Change, DbErr> {
    let changed = if local {
        let changed: bool = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT federation_withdraw_local($1) AS changed",
                [trk.id.into()],
            ))
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("publication".into()))?
            .try_get("", "changed")?;
        if changed && trk.file_hash.is_none() {
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO federation_withdrawn_fingerprints(track_id, expected_sha256)
                 SELECT t.id,m.sha256 FROM tracks t LEFT JOIN federation_managed_originals m ON m.file_path=t.file_path
                 WHERE t.id=$1 AND (t.content_hash IS NOT NULL OR m.sha256 IS NOT NULL) ON CONFLICT DO NOTHING",
                [trk.id.into()],
            )).await?;
        }
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE federation_file_cleanup SET retry_at=now() WHERE file_path=$1",
            [trk.file_path.as_str().into()],
        ))
        .await?;
        changed
    } else {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT federation_project_track(t, true) FROM tracks t WHERE id=$1",
            [trk.id.into()],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO federation_hidden_publications(track_id) VALUES($1) ON CONFLICT DO NOTHING",
            [trk.id.into()])).await?.rows_affected() != 0
    };
    let hashes = tx.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT content_hash FROM publication_versions WHERE track_id=$1 AND content_hash IS NOT NULL \
         UNION SELECT content_hash FROM tracks WHERE id=$1 AND content_hash IS NOT NULL",
        [trk.id.into()])).await?.into_iter().map(|r| r.try_get("", "content_hash")).collect::<Result<_,_>>()?;
    Ok(Change {
        id: trk.id,
        hashes,
        local,
        changed,
    })
}

struct VerifiedFile {
    version: MediaVersion,
    size: u64,
    sha256: String,
    blob: Option<String>,
}

// Read one stable version in bounded windows, outside the catalogue writer lock.
async fn fingerprint(
    state: &AppState,
    path: &str,
    need_blob: bool,
) -> Result<VerifiedFile, StorageError> {
    let file = state.storage.open_media(path).await?;
    let version = file.version();
    let size = file.size();
    let mut sha = Sha256::new();
    let mut blob = need_blob.then(soundtime_p2p::BlobHasher::new);
    if size > 0 {
        let mut reader = file.range(0, size).await?;
        while let Some(bytes) = reader.next_chunk().await? {
            sha.update(&bytes);
            if let Some(blob) = &mut blob {
                blob.update(&bytes);
            }
        }
    }
    Ok(VerifiedFile {
        version,
        size,
        sha256: format!("{:x}", sha.finalize()),
        blob: blob.map(|b| b.finalize().to_hex().to_string()),
    })
}

async fn fingerprint_one(state: &AppState) -> Result<bool, DbErr> {
    let Some(job) = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "WITH due AS (SELECT track_id FROM federation_withdrawn_fingerprints
         WHERE retry_at<=now() ORDER BY retry_at,track_id LIMIT 1 FOR UPDATE SKIP LOCKED)
         UPDATE federation_withdrawn_fingerprints j SET retry_at=now()+interval '10 minutes'
         FROM due, tracks t WHERE j.track_id=due.track_id AND t.id=j.track_id
         RETURNING t.id,t.file_path,t.file_size,t.file_hash,t.content_hash,j.expected_sha256",
        ))
        .await?
    else {
        return Ok(false);
    };
    let id: Uuid = job.try_get("", "id")?;
    let path: String = job.try_get("", "file_path")?;
    let known_sha: Option<String> = job.try_get("", "file_hash")?;
    if known_sha.is_none() {
        let blob: Option<String> = job.try_get("", "content_hash")?;
        let expected: Option<String> = job.try_get("", "expected_sha256")?;
        let size: i64 = job.try_get("", "file_size")?;
        let verified = match fingerprint(state, &path, blob.is_some()).await {
            Ok(file)
                if i64::try_from(file.size).ok() == Some(size)
                    && (blob.is_some() || expected.is_some())
                    && file.blob == blob
                    && expected.as_ref().is_none_or(|sha| sha == &file.sha256) =>
            {
                file
            }
            _ => return Ok(true),
        };
        let tx = crate::storage_catalog::begin_write(&state.db).await?;
        let current = state.storage.open_media(&path).await;
        if !current.is_ok_and(|f| f.version() == verified.version) {
            return Ok(true);
        }
        tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE tracks SET file_hash=$3 WHERE id=$1 AND file_path=$2 AND file_hash IS NULL
             AND EXISTS(SELECT FROM track_publications p WHERE p.track_id=$1 AND p.state='withdrawn')",
            [id.into(), path.into(), verified.sha256.into()])).await?;
        tx.commit().await?;
    }
    // Also handle a scanner which supplied the missing digest before this job claimed it.
    state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "WITH finished AS (
            DELETE FROM federation_withdrawn_fingerprints j USING tracks t
            WHERE j.track_id=$1 AND t.id=j.track_id AND t.file_hash IS NOT NULL RETURNING j.track_id)
         INSERT INTO federation_file_cleanup(track_id,file_path,expected_sha256)
         SELECT t.id,t.file_path,t.file_hash FROM finished f JOIN tracks t ON t.id=f.track_id
         JOIN federation_managed_originals m ON m.file_path=t.file_path AND m.sha256=t.file_hash
         JOIN track_publications p ON p.track_id=t.id AND p.state='withdrawn' ON CONFLICT DO NOTHING",
        [id.into()])).await?;
    Ok(true)
}

async fn clean_one(state: &AppState) -> Result<bool, DbErr> {
    let Some(job) = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "WITH due AS (SELECT j.track_id FROM federation_file_cleanup j WHERE j.retry_at<=now()
         AND NOT EXISTS(SELECT FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id
         WHERE t.file_path=j.file_path AND p.state IS DISTINCT FROM 'withdrawn')
         ORDER BY j.retry_at,j.track_id LIMIT 1 FOR UPDATE SKIP LOCKED)
         UPDATE federation_file_cleanup j SET retry_at=now()+interval '10 minutes' FROM due
         WHERE j.track_id=due.track_id RETURNING j.track_id,j.file_path,j.expected_sha256",
        ))
        .await?
    else {
        return Ok(false);
    };
    let id: Uuid = job.try_get("", "track_id")?;
    let path: String = job.try_get("", "file_path")?;
    let expected: String = job.try_get("", "expected_sha256")?;
    let verified = fingerprint(state, &path, false).await;
    let tx = crate::storage_catalog::begin_write(&state.db).await?;
    if tx.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT track_id FROM federation_file_cleanup WHERE track_id=$1 AND file_path=$2 AND expected_sha256=$3 FOR UPDATE",
        [id.into(), path.as_str().into(), expected.as_str().into()])).await?.is_none() {
        return Ok(true);
    }
    let shared = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT EXISTS(SELECT FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id \
         WHERE t.file_path=$1 AND p.state IS DISTINCT FROM 'withdrawn') AS shared",
            [path.as_str().into()],
        ))
        .await?
        .expect("EXISTS returns a row")
        .try_get::<bool>("", "shared")?;
    let finished = if shared {
        false
    } else {
        match verified {
            Ok(file) => match state.storage.open_media(&path).await {
                Ok(now) if now.version() == file.version => {
                    file.sha256 != expected
                        || state
                            .storage
                            .delete_file(&path)
                            .await
                            .map_err(
                                |e| tracing::warn!(%id, %e, "original file cleanup will retry"),
                            )
                            .is_ok()
                }
                _ => false,
            },
            Err(_) => matches!(state.storage.file_exists(&path).await, Ok(false)),
        }
    };
    let statement = if finished {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM federation_managed_originals WHERE file_path=$1 AND sha256=$2",
            [path.as_str().into(), expected.into()],
        ))
        .await?;
        "DELETE FROM federation_file_cleanup WHERE track_id=$1"
    } else {
        "UPDATE federation_file_cleanup SET retry_at=now()+interval '10 minutes' WHERE track_id=$1"
    };
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        statement,
        [id.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub fn spawn_cleanup(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let fingerprinted = fingerprint_one(&state)
                .await
                .map_err(|e| tracing::warn!(%e, "withdrawn fingerprint will retry"))
                .unwrap_or(false);
            match clean_one(&state).await {
                Ok(true) => continue,
                Ok(false) if fingerprinted => continue,
                Ok(false) => {}
                Err(e) => tracing::warn!(%e, "original file cleanup will retry"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests;
