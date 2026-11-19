//! Publication authority and cleanup of original files owned by this instance.
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, Statement};
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
    state: &AppState,
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
            // Capture a legacy original's fingerprint while its path still belongs to this publication.
            // Failed/missing storage never rolls back the logical withdrawal.
            if let Ok(hash) = state.storage.hash_file(&trk.file_path).await {
                tx.execute(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "UPDATE tracks SET file_hash=$2 WHERE id=$1 AND file_hash IS NULL",
                    [trk.id.into(), hash.into()],
                ))
                .await?;
            }
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

async fn clean_one(state: &AppState) -> Result<bool, DbErr> {
    let due = state
        .db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT EXISTS(SELECT FROM federation_file_cleanup WHERE retry_at<=now()) AS due",
        ))
        .await?
        .expect("EXISTS returns a row")
        .try_get::<bool>("", "due")?;
    if !due {
        return Ok(false);
    }
    let tx = crate::storage_catalog::begin_write(&state.db).await?;
    let Some(job) = tx.query_one(Statement::from_string(DbBackend::Postgres,
        "SELECT track_id,file_path,expected_sha256 FROM federation_file_cleanup WHERE retry_at<=now() ORDER BY retry_at,track_id LIMIT 1 FOR UPDATE")).await? else {
        tx.commit().await?;
        return Ok(false);
    };
    let id: Uuid = job.try_get("", "track_id")?;
    let path: String = job.try_get("", "file_path")?;
    let expected: String = job.try_get("", "expected_sha256")?;
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
        match state.storage.file_exists(&path).await {
            Ok(false) => true,
            Ok(true) => match state.storage.hash_file(&path).await {
                Ok(hash) if hash != expected => true, // A different file now owns this path.
                Ok(_) => state
                    .storage
                    .delete_file(&path)
                    .await
                    .map_err(|e| tracing::warn!(%id, %e, "original file cleanup will retry"))
                    .is_ok(),
                Err(e) => {
                    tracing::warn!(%id, %e, "could not verify original file");
                    false
                }
            },
            Err(e) => {
                tracing::warn!(%id, %e, "original storage unavailable");
                false
            }
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
            match clean_one(&state).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => tracing::warn!(%e, "original file cleanup will retry"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}
