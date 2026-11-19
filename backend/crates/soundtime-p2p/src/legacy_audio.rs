//! Compatibility reads are scoped to a saved UUID; unversioned bytes never
//! become an origin claim or an exportable publication offer.
use super::{LegacyFetch, P2pError, P2pNode};
use bytes::Bytes;
use futures_util::{FutureExt, StreamExt};
use iroh::{EndpointAddr, EndpointId};
use iroh_blobs::Hash;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use soundtime_db::media_access;
use std::{sync::Arc, time::Duration};
use tracing::warn;
use uuid::Uuid;

fn sql(s: &str, values: impl IntoIterator<Item = sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Postgres, s, values)
}
impl P2pNode {
    async fn legacy_grants(&self, id: Uuid, hash: Hash) -> Result<(bool, bool), P2pError> {
        let row=self.db.query_one(sql("SELECT
                (p.state='unresolved' AND t.content_hash=$2 AND t.file_path='p2p://'||$2) IS TRUE AS legacy,
                ((p.state='unresolved' AND cp.track_id IS NOT NULL AND cp.content_hash=$2) OR
                 (p.state='active' AND v.content_hash=$2 AND (own.is_local OR confirmed.track_id IS NOT NULL))) IS TRUE AS retained
            FROM tracks t JOIN track_publications p ON p.track_id=t.id
            LEFT JOIN federation_legacy_copies cp ON cp.track_id=t.id
            LEFT JOIN federation_catalogs own ON own.id=p.catalog_id
            LEFT JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
            LEFT JOIN federation_cached_publications confirmed ON confirmed.track_id=p.track_id
              AND confirmed.revision=p.revision AND confirmed.content_hash=v.content_hash
            WHERE t.id=$1 AND p.state IN ('active','unresolved')
             AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=t.id)
             AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=$2)",
             [id.into(),hash.to_string().into()])).await?.ok_or_else(||P2pError::TrackNotFound(id.to_string()))?;
        let raw = row.try_get::<bool>("", "legacy")?;
        if raw && !media_access::allows_legacy_hash(&self.db, &hash.to_string()).await? {
            return Err(P2pError::TrackNotFound(id.to_string()));
        }
        Ok((raw, row.try_get("", "retained")?))
    }

    pub async fn get_or_fetch_track(
        self: &Arc<Self>,
        id: Uuid,
        hash: Hash,
    ) -> Result<(Bytes, Option<media_access::ReadGuard>), P2pError> {
        let raw = media_access::ReadGuard::legacy_hash(&hash.to_string());
        let (legacy, retained) = self.legacy_grants(id, hash).await?;
        if retained {
            if let Ok(bytes) = self.get_local_track(hash).await {
                if self.legacy_grants(id, hash).await?.1 {
                    return Ok((bytes, legacy.then_some(raw)));
                }
            }
        }
        if !legacy {
            return Err(P2pError::TrackNotFound(id.to_string()));
        }
        let key = (id, hash);
        let transfer = {
            let mut active = self.legacy_fetches.lock().expect("legacy fetches");
            active.retain(|_, f| f.upgrade().is_some());
            if let Some(running) = active.get(&key).and_then(|f| f.upgrade()) {
                running
            } else {
                if active.len() >= 32 {
                    return Err(P2pError::Connection("legacy fetch budget exhausted".into()));
                }
                let node = self.clone();
                let job: LegacyFetch = async move {
                    node.fetch_legacy_publication(id, hash)
                        .await
                        .map_err(|e| Arc::<str>::from(e.to_string()))
                }
                .boxed();
                let job = job.shared();
                active.insert(key, job.downgrade().expect("new shared fetch"));
                job
            }
        };
        let (bytes, source) = transfer
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;
        let (legacy, retained) = self.legacy_grants(id, hash).await?;
        if !legacy || (!retained && !self.legacy_providers(id, hash).await?.contains(&source)) {
            return Err(P2pError::TrackNotFound(id.to_string()));
        }
        Ok((bytes, Some(raw)))
    }

    async fn legacy_providers(&self, id: Uuid, hash: Hash) -> Result<Vec<String>, P2pError> {
        let rows = self
            .db
            .query_all(sql(
                "SELECT DISTINCT s.provider FROM federation_track_providers s
            JOIN tracks t ON t.id=s.track_id AND t.content_hash=$2 AND t.file_path='p2p://'||$2
            JOIN track_publications p ON p.track_id=t.id AND p.state='unresolved'
            WHERE t.id=$1 AND s.allowed AND s.online AND (NOT $3 OR s.followed) AND s.provider<>$4
            ORDER BY s.provider LIMIT 8",
                [
                    id.into(),
                    hash.to_string().into(),
                    self.explicit_catalog().into(),
                    self.node_id().to_string().into(),
                ],
            ))
            .await?;
        Ok(rows
            .iter()
            .map(|r| r.try_get("", "provider"))
            .collect::<Result<Vec<_>, _>>()?)
    }

    async fn fetch_legacy_publication(
        &self,
        id: Uuid,
        hash: Hash,
    ) -> Result<(Bytes, String), P2pError> {
        for source in self.legacy_providers(id, hash).await? {
            let Ok(peer) = source.parse::<EndpointId>() else {
                continue;
            };
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                self.fetch_track_from_peer(EndpointAddr::new(peer), hash),
            )
            .await;
            let Ok(Ok(data)) = result else { continue };
            if Hash::new(&data) != hash || !self.legacy_providers(id, hash).await?.contains(&source)
            {
                continue;
            }
            if !self.legacy_grants(id, hash).await?.0 {
                return Err(P2pError::TrackNotFound(id.to_string()));
            }
            let store = async {
                if let Some(mut writer) =
                    self.reserve_cached_version(hash, data.len() as u64).await?
                {
                    for chunk in data.chunks(65536) {
                        writer.append(chunk).await?;
                    }
                    let _guard = writer.commit_guard(false).await?;
                    self.bind_legacy_copy(id, hash).await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            if let Err(e) = store.await {
                warn!(%id,%e,"legacy cache admission skipped");
            }
            return Ok((data, source));
        }
        Err(P2pError::TrackNotFound(id.to_string()))
    }

    async fn bind_legacy_copy(&self, id: Uuid, hash: Hash) -> Result<(), P2pError> {
        self.db
            .execute(sql(
                "INSERT INTO federation_legacy_copies(track_id,content_hash)
           SELECT t.id,t.content_hash FROM tracks t JOIN track_publications p ON p.track_id=t.id
           WHERE t.id=$1 AND p.state='unresolved' AND t.content_hash=$2 AND t.file_path='p2p://'||$2
           AND NOT EXISTS(SELECT FROM federation_legacy_denied_hashes x WHERE x.content_hash=$2)
           ON CONFLICT(track_id) DO UPDATE SET content_hash=excluded.content_hash",
                [id.into(), hash.to_string().into()],
            ))
            .await?;
        Ok(())
    }

    pub(super) fn start_legacy_cache_recovery(self: &Arc<Self>) {
        let node = self.clone();
        let mut evicted = self.blob_cache.evictions();
        let mut stop = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = node.recover_legacy_tags().await {
                warn!(%e,"old cache tag recovery failed");
            }
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            let mut after = None;
            loop {
                tokio::select! {
                    _=stop.changed()=>break,
                    message=evicted.recv()=>match message{
                        Ok(hash)=>if let Err(e)=node.forget_missing_legacy(hash).await{warn!(%hash,%e,"legacy eviction reconciliation deferred");},
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>after=None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                    },
                    _=tick.tick()=>if let Err(e)=node.check_legacy_page(&mut after).await{warn!(%e,"legacy cache reconciliation deferred");},
                }
            }
        });
    }

    async fn recover_legacy_tags(&self) -> anyhow::Result<()> {
        let mut tags = Box::pin(self.blob_store.tags().list_prefix("p2p-cache-").await?);
        while let Some(tag) = tags.next().await {
            let tag = tag?;
            if tag.name.as_ref() != format!("p2p-cache-{}", tag.hash).as_bytes()
                || !self.blob_store.blobs().has(tag.hash).await?
            {
                continue;
            }
            let rows=self.db.query_all(sql("SELECT DISTINCT t.id FROM tracks t JOIN remote_tracks r ON r.local_track_id=t.id
              WHERE t.content_hash=$1 AND t.file_path='p2p://'||$1 AND r.instance_domain LIKE 'p2p://%'
                AND r.remote_uri=r.instance_domain||'/'||$1 LIMIT 2",[tag.hash.to_string().into()])).await?;
            if let [row] = &rows[..] {
                self.bind_legacy_copy(row.try_get("", "id")?, tag.hash)
                    .await?;
            }
        }
        Ok(())
    }

    async fn forget_missing_legacy(&self, hash: Hash) -> anyhow::Result<()> {
        if !self.has_blob(hash).await {
            self.db
                .execute(sql(
                    "DELETE FROM federation_legacy_copies WHERE content_hash=$1",
                    [hash.to_string().into()],
                ))
                .await?;
        }
        Ok(())
    }

    async fn check_legacy_page(&self, after: &mut Option<Uuid>) -> anyhow::Result<()> {
        let rows = self
            .db
            .query_all(sql(
                "SELECT track_id,content_hash FROM federation_legacy_copies
            WHERE $1::uuid IS NULL OR track_id>$1 ORDER BY track_id LIMIT 64",
                [(*after).into()],
            ))
            .await?;
        for row in &rows {
            let hash = row.try_get::<String>("", "content_hash")?.parse()?;
            self.forget_missing_legacy(hash).await?;
            *after = Some(row.try_get("", "track_id")?);
        }
        if rows.len() < 64 {
            *after = None
        }
        Ok(())
    }
}
