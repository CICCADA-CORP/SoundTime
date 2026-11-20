//! Administrative retention of complete authorized publications and albums.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use soundtime_db::{media_access::ReadGuard, AppState};
use soundtime_p2p::{
    blob_cache::{CacheRead, CacheWriter},
    range_transport::{WindowRequest, WINDOW_BYTES},
    BlobHash, P2pNode,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

type Failure = (StatusCode, Json<Value>);
fn fail(status: StatusCode, msg: &str) -> Failure {
    (status, Json(json!({"error":msg})))
}
fn db(e: sea_orm::DbErr) -> Failure {
    tracing::error!(%e,"cache pin database error");
    fail(StatusCode::SERVICE_UNAVAILABLE, "Cache policy unavailable")
}
fn io(e: impl std::fmt::Display) -> Failure {
    tracing::warn!(%e,"cache pin unavailable");
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "Verified audio unavailable",
    )
}
fn node(state: &AppState) -> Result<Arc<P2pNode>, Failure> {
    state
        .p2p
        .as_ref()
        .and_then(|n| n.clone().downcast::<P2pNode>().ok())
        .filter(|n| n.range_streaming())
        .ok_or_else(|| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "Verified range streaming is disabled",
            )
        })
}
fn scope_valid(scope: &str) -> Result<(), Failure> {
    if matches!(scope, "tracks" | "albums") {
        Ok(())
    } else {
        Err(fail(StatusCode::NOT_FOUND, "Unknown pin collection"))
    }
}
async fn lock(db_conn: &impl ConnectionTrait) -> Result<(), sea_orm::DbErr> {
    db_conn
        .execute_unprepared("SELECT pg_advisory_xact_lock(1937011565,4)")
        .await?;
    Ok(())
}

enum Admission {
    Existing(CacheRead),
    Pending(CacheWriter),
}

#[derive(serde::Deserialize)]
pub struct Quota {
    pub maximum_bytes: u64,
}

pub async fn set_quota(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Quota>,
) -> Result<Json<Value>, Failure> {
    tokio::spawn(async move { apply_quota(state, body.maximum_bytes).await })
        .await
        .map_err(io)?
}

async fn apply_quota(state: Arc<AppState>, limit: u64) -> Result<Json<Value>, Failure> {
    let node = state
        .p2p
        .as_ref()
        .and_then(|n| n.clone().downcast::<P2pNode>().ok())
        .ok_or_else(|| fail(StatusCode::SERVICE_UNAVAILABLE, "P2P cache unavailable"))?;
    let tx = state.db.begin().await.map_err(db)?;
    lock(&tx).await.map_err(db)?;
    let cache = Arc::clone(node.blob_cache());
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO instance_settings(id,key,value,updated_at) VALUES (gen_random_uuid(),'p2p_cache_quota_bytes',$1,now())
         ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=now()",[limit.to_string().into()])).await.map_err(db)?;
    let ceiling = node
        .blob_cache()
        .usage()
        .map_err(io)?
        .configured_ceiling_bytes;
    tokio::task::spawn_blocking(move || cache.set_maximum(limit))
        .await
        .map_err(io)?
        .map_err(|e| fail(StatusCode::CONFLICT, &e.to_string()))?;
    if let Err(e) = tx.commit().await {
        // A lost commit response can be ambiguous. Re-read the persisted value before restoring it.
        let read = state.db.begin().await.map_err(db)?;
        lock(&read).await.map_err(db)?;
        let row = read
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT value FROM instance_settings WHERE key='p2p_cache_quota_bytes'",
            ))
            .await
            .map_err(db)?;
        let actual = match row {
            Some(row) => row
                .try_get::<String>("", "value")
                .map_err(db)?
                .parse::<u64>()
                .map_err(io)?,
            None => ceiling,
        };
        node.blob_cache().restore_maximum(actual).map_err(io)?;
        read.commit().await.map_err(db)?;
        return Err(db(e));
    }
    Ok(Json(json!({"maximum_bytes":limit})))
}

pub async fn pin(
    State(state): State<Arc<AppState>>,
    Path((scope, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, Failure> {
    scope_valid(&scope)?;
    let node = node(&state)?;
    let rows=state.db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
      "SELECT t.id,p.state,coalesce(c.is_local, false) AS local,
              EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=t.id) AS missing,
              EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=t.id)
              OR EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash IN (t.content_hash,v.content_hash)) AS hidden
       FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id LEFT JOIN federation_catalogs c ON c.id=p.catalog_id
       LEFT JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
       WHERE ($1='tracks' AND t.id=$2) OR ($1='albums' AND t.album_id=$2) ORDER BY t.id",
      [scope.as_str().into(),id.into()])).await.map_err(db)?;
    if rows.is_empty() {
        return Err(fail(StatusCode::NOT_FOUND, "No tracks found"));
    }
    let mut tracks = Vec::new();
    let mut admissions = HashMap::new();
    let mut originals = 0;
    for row in rows {
        let track: Uuid = row.try_get("", "id").map_err(db)?;
        if row
            .try_get::<Option<String>>("", "state")
            .map_err(db)?
            .as_deref()
            != Some("active")
            || row.try_get::<bool>("", "hidden").map_err(db)?
        {
            return Err(fail(
                StatusCode::CONFLICT,
                "Collection contains an unavailable publication",
            ));
        }
        if row.try_get::<bool>("", "local").map_err(db)?
            && !row.try_get::<bool>("", "missing").map_err(db)?
        {
            originals += 1;
            continue;
        }
        let m = node
            .range_manifest(track)
            .await
            .map_err(io)?
            .ok_or_else(|| fail(StatusCode::CONFLICT, "Publication has no verified version"))?;
        if !node.can_retain_version(track, &m).await.map_err(io)? {
            return Err(fail(
                StatusCode::CONFLICT,
                "No authorized provider for this version",
            ));
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = admissions.entry(m.hash) {
            let admission = if let Some(guard) = node.blob_cache().read_guard(m.hash) {
                Admission::Existing(guard)
            } else {
                let writer = node
                    .reserve_cached_version(m.hash, m.file_size)
                    .await
                    .map_err(io)?;
                match writer {
                    Some(w) => Admission::Pending(w),
                    None if node.blob_cache().pending(m.hash) => {
                        return Err(fail(
                            StatusCode::CONFLICT,
                            "File already being retained; retry shortly",
                        ))
                    }
                    None => {
                        return Err(fail(
                            StatusCode::INSUFFICIENT_STORAGE,
                            "Insufficient cache capacity or free disk space",
                        ))
                    }
                }
            };
            slot.insert(admission);
        }
        tracks.push((track, m));
    }
    // Allocate the complete album budget before reading its first network window.
    let mut guards = Vec::new();
    for (track, m) in &tracks {
        match admissions.remove(&m.hash) {
            Some(Admission::Existing(guard)) => guards.push(guard),
            Some(Admission::Pending(mut writer)) => {
                let access = ReadGuard::track(*track);
                let mut req = m.clone();
                req.offset = 0;
                while req.offset < req.file_size {
                    req.length = (req.file_size - req.offset).min(u64::from(WINDOW_BYTES)) as u32;
                    let bytes = access
                        .during(node.verified_range(*track, req.clone()))
                        .await
                        .map_err(io)?
                        .map_err(io)?;
                    writer.append(&bytes).await.map_err(io)?;
                    req.offset += bytes.len() as u64;
                }
                guards.push(
                    access
                        .during(writer.commit_guard(false))
                        .await
                        .map_err(io)?
                        .map_err(io)?,
                );
            }
            None => {}
        }
    }

    let tx = state.db.begin().await.map_err(db)?;
    lock(&tx).await.map_err(db)?;
    tx.execute_unprepared("SELECT pg_advisory_xact_lock(1937011565,1)")
        .await
        .map_err(db)?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM federation_cache_pins WHERE scope=$1 AND scope_id=$2",
        [scope.as_str().into(), id.into()],
    ))
    .await
    .map_err(db)?;
    for (track, m) in &tracks {
        record(&tx, &scope, id, *track, m).await?;
        if !node.pin_cached(m.hash, true).await.map_err(io)? {
            return Err(fail(StatusCode::CONFLICT, "Cache changed; retry pin"));
        }
    }
    tx.commit().await.map_err(db)?;
    // Offers are per publication: sharing a hash with a different publication grants no authority.
    let mut offered = 0;
    for (track, m) in &tracks {
        if node.offer_cached_publication(*track, m.hash).await.is_ok() {
            offered += 1;
        }
    }
    Ok(Json(
        json!({"scope":scope,"id":id,"pinned_publications":tracks.len(),"local_originals":originals,"offers":offered}),
    ))
}

async fn record(
    tx: &DatabaseTransaction,
    scope: &str,
    id: Uuid,
    track: Uuid,
    m: &WindowRequest,
) -> Result<(), Failure> {
    let row=tx.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT p.track_id FROM track_publications p JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
         WHERE p.track_id=$1 AND p.revision=$2 AND p.state='active' AND v.content_hash=$3
           AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
           AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=v.content_hash)
         FOR SHARE OF p",
        [track.into(),(m.revision as i64).into(),m.hash.to_string().into()])).await.map_err(db)?;
    if row.is_none() {
        return Err(fail(StatusCode::CONFLICT, "Publication changed during pin"));
    }
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_cache_pins(scope,scope_id,track_id,revision,content_hash) VALUES ($1,$2,$3,$4,$5)",
        [scope.into(),id.into(),track.into(),(m.revision as i64).into(),m.hash.to_string().into()])).await.map_err(db)?;
    Ok(())
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    Path((scope, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, Failure> {
    scope_valid(&scope)?;
    if node(&state).is_err() {
        return Ok(Json(json!({"eligible":false,"pinned":false})));
    }
    let row=state.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
       "SELECT count(*) AS total, count(pin.track_id) AS held FROM tracks t
        JOIN track_publications p ON p.track_id=t.id JOIN federation_catalogs c ON c.id=p.catalog_id
        LEFT JOIN federation_cache_pins pin ON pin.scope=$1 AND pin.scope_id=$2 AND pin.track_id=t.id AND pin.revision=p.revision
        WHERE (($1='tracks' AND t.id=$2) OR ($1='albums' AND t.album_id=$2)) AND p.state='active'
        AND (NOT c.is_local OR EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=t.id))",
       [scope.as_str().into(),id.into()])).await.map_err(db)?.expect("aggregate returns one row");
    let total: i64 = row.try_get("", "total").map_err(db)?;
    let held: i64 = row.try_get("", "held").map_err(db)?;
    Ok(Json(
        json!({"eligible":total>0,"pinned":held>0 && held==total}),
    ))
}

pub async fn unpin(
    State(state): State<Arc<AppState>>,
    Path((scope, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, Failure> {
    scope_valid(&scope)?;
    let tx = state.db.begin().await.map_err(db)?;
    lock(&tx).await.map_err(db)?;
    let result = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM federation_cache_pins WHERE scope=$1 AND scope_id=$2",
            [scope.as_str().into(), id.into()],
        ))
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    if let Ok(node) = node(&state) {
        reconcile(&state, &node).await.map_err(io)?;
    }
    Ok(Json(
        json!({"released_publications":result.rows_affected()}),
    ))
}

async fn reconcile(state: &AppState, node: &P2pNode) -> anyhow::Result<()> {
    let tx = state.db.begin().await?;
    lock(&tx).await?;
    let rows = tx
        .query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT DISTINCT content_hash FROM federation_cache_pins",
        ))
        .await?;
    let wanted: HashSet<BlobHash> = rows
        .into_iter()
        .map(|row| -> anyhow::Result<_> { Ok(row.try_get::<String>("", "content_hash")?.parse()?) })
        .collect::<anyhow::Result<_>>()?;
    let protected: HashSet<_> = node.blob_cache().pinned().into_iter().collect();
    for hash in wanted.difference(&protected) {
        node.pin_cached(*hash, true).await?;
    }
    for hash in protected.difference(&wanted) {
        node.pin_cached(*hash, false).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn spawn(state: Arc<AppState>) {
    let Ok(node) = node(&state) else {
        return;
    };
    tokio::spawn(async move {
        loop {
            if let Err(e) = reconcile(&state, &node).await {
                tracing::warn!(%e,"cache pin reconciliation deferred");
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}
