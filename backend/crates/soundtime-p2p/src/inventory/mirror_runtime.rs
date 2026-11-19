//! Mirror leases are issued only after a complete publication-specific cache admission.
use super::{P2pMessage, P2pNode};
use crate::inventory::{
    self,
    lease::{self, Offer},
    source::{Request, Response},
};
use anyhow::{ensure, Context, Result};
use futures_util::{stream, StreamExt};
use iroh::EndpointId;
use iroh_blobs::Hash;
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

fn sql(query: &str, values: impl IntoIterator<Item = sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Postgres, query, values)
}

impl P2pNode {
    pub fn share_cached(&self) -> bool {
        self.versioned_catalog() && self._config.share_cached && self.range_streaming()
    }

    /// The caller verified all bytes in a complete admission for this publication.
    /// Even with network sharing off, this association permits future local reads.
    pub async fn offer_cached_publication(self: &Arc<Self>, track: Uuid, hash: Hash) -> Result<()> {
        ensure!(
            self.cache_contains(hash),
            "completed file no longer in cache"
        );
        let tx = self.db.begin().await?;
        let row=tx.query_one(sql("SELECT p.revision FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
            JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
            WHERE p.track_id=$1 AND p.state='active' AND NOT c.is_local AND v.content_hash=$2
            AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
            AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=v.content_hash) FOR UPDATE OF p",
            [track.into(),hash.to_string().into()])).await?.context("publication unavailable for admission")?;
        let revision: i64 = row.try_get("", "revision")?;
        tx.execute(sql("INSERT INTO federation_cached_publications(track_id,revision,content_hash) VALUES($1,$2,$3)
             ON CONFLICT(track_id,revision) DO NOTHING", [track.into(),revision.into(),hash.to_string().into()])).await?;
        tx.commit().await?;
        self.revalidate_cached(track, revision).await
    }

    async fn revalidate_cached(self: &Arc<Self>, track: Uuid, revision: i64) -> Result<()> {
        let tx = self.db.begin().await?;
        let Some(row)=tx.query_one(sql("SELECT cache.content_hash,s.envelope,c.node_id,p.origin_publication_id,
                p.state='active' AND p.revision=cache.revision AND v.content_hash=cache.content_hash
                AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
                AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=cache.content_hash) AS authorized
                FROM federation_cached_publications cache JOIN track_publications p ON p.track_id=cache.track_id
                JOIN federation_catalogs c ON c.id=p.catalog_id JOIN publication_versions v ON v.track_id=cache.track_id AND v.revision=cache.revision
                LEFT JOIN federation_signed_publications s ON s.track_id=cache.track_id
                WHERE cache.track_id=$1 AND cache.revision=$2 FOR UPDATE OF cache,p",
            [track.into(),revision.into()])).await? else {return Ok(())};
        let hash: Hash = row.try_get::<String>("", "content_hash")?.parse()?;
        let present = self.cache_contains(hash) && row.try_get::<bool>("", "authorized")?;
        let existing=tx.query_one(sql("SELECT active,expires_at>now()+interval '20 minutes' AS fresh,manifest
            FROM federation_mirror_leases WHERE track_id=$1 AND revision=$2 AND provider=$3 FOR UPDATE",
            [track.into(), revision.into(),self.node_id().to_string().into()])).await?;
        let active = present && self.share_cached();
        if !present {
            tx.execute(sql(
                "DELETE FROM federation_cached_publications WHERE track_id=$1 AND revision=$2",
                [track.into(), revision.into()],
            ))
            .await?;
        }
        if existing.as_ref().is_some_and(|r| {
            r.try_get::<bool>("", "active").ok() == Some(active)
                && (!active || r.try_get::<bool>("", "fresh").ok() == Some(true))
        }) || (existing.is_none() && !active)
        {
            tx.commit().await?;
            return Ok(());
        }
        // A retraction uses the version actually offered, even after the origin
        // replaces its manifest or withdraws the publication.
        let proof: serde_json::Value = if let Some(prior) = existing {
            prior.try_get("", "manifest")?
        } else {
            row.try_get::<Option<serde_json::Value>>("", "envelope")?
                .context("missing signed publication manifest")?
        };
        let manifest: inventory::SignedPublication = serde_json::from_value(proof)?;
        ensure!(
            manifest.manifest.revision == revision
                && manifest
                    .manifest
                    .media
                    .as_ref()
                    .is_some_and(|m| m.hash == hash.to_string()),
            "local proof does not match cache grant"
        );
        manifest.verify()?;
        let offer = Offer::issue(
            self.endpoint.secret_key(),
            manifest,
            lease::tick(&tx).await?,
            active,
        )?;
        lease::store(&tx, track, &offer, true).await?;
        tx.commit().await?;
        self.notify_mirror_change(offer).await;
        Ok(())
    }

    pub(super) async fn notify_mirror_change(self: &Arc<Self>, offer: Offer) {
        let Ok(permit) = self.catalogs.notice_slot() else {
            return;
        };
        let node = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let origin = offer.provider.lease.origin;
            let peers = node.registry.online_peers().await;
            let msg = &P2pMessage::MirrorOffer(offer);
            let node = &node;
            stream::iter(peers)
                .for_each_concurrent(4, |peer| async move {
                    let Ok(id) = peer.node_id.parse::<EndpointId>() else {
                        return;
                    };
                    if !node.catalogs.supports(id).await {
                        return;
                    }
                    if id != origin && !node.subscribers.read().await.contains(&id) {
                        return;
                    }
                    if !matches!(
                        crate::peer_policy::permits_export(&node.db, &id.to_string()).await,
                        Ok(true)
                    ) {
                        return;
                    }
                    if let Err(e) = node.send_message_to_peer(id, msg).await {
                        tracing::debug!(%id,%e,"mirror notice retained for pull");
                    }
                })
                .await;
        });
    }

    pub(super) async fn pull_mirrors(&self, id: EndpointId) -> Result<()> {
        let row=self.db.query_one(sql("SELECT mirror_cursor FROM federation_peer_policies WHERE node_id=$1 AND is_followed AND NOT is_banned",
            [id.to_string().into()])).await?.context("catalogue no longer followed")?;
        let mut after: i64 = row.try_get("", "mirror_cursor")?;
        for _ in 0..4 {
            let Response::Mirrors(rows) = self.catalog_rpc(id, Request::Mirrors { after }).await?
            else {
                anyhow::bail!("unexpected mirror index")
            };
            ensure!(rows.len() <= 16, "oversized mirror index");
            let last = rows.len() < 16;
            for event in rows {
                ensure!(event.cursor > after, "unordered mirror journal");
                lease::accept(
                    &self.db,
                    self.node_id(),
                    id,
                    &event.offer,
                    Some(event.cursor),
                )
                .await?;
                after = event.cursor;
            }
            if last {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn start_mirror_worker(self: &Arc<Self>) {
        let node = self.clone();
        let mut shutdown = node.shutdown_tx.subscribe();
        let mut evicted = node.blob_cache.evictions();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut after = None;
            loop {
                tokio::select! {
                    _=shutdown.changed()=>break,
                    event=evicted.recv()=>match event{
                        Ok(hash)=>if let Err(e)=node.refresh_cached_hash(hash).await{tracing::warn!(%hash,%e,"cache eviction publication refresh deferred");},
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>after=None,
                        Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                    },
                    _=tick.tick()=>if let Err(e)=node.refresh_cached_page(&mut after).await{tracing::warn!(%e,"mirror reconciliation deferred");},
                }
            }
        });
    }

    async fn refresh_cached_hash(self: &Arc<Self>, hash: Hash) -> Result<()> {
        // Concurrent re-admission is checked inside the publication transaction.
        let mut after = None;
        loop {
            let rows = self
                .db
                .query_all(sql(
                    "SELECT track_id,revision FROM federation_cached_publications
                WHERE content_hash=$1 AND ($2::uuid IS NULL OR (track_id,revision)>($2,$3))
                ORDER BY track_id,revision LIMIT 64",
                    [
                        hash.to_string().into(),
                        after.map(|(id, _): (Uuid, i64)| id).into(),
                        after.map(|(_, v)| v).into(),
                    ],
                ))
                .await?;
            for row in &rows {
                let id = row.try_get("", "track_id")?;
                let revision = row.try_get("", "revision")?;
                self.revalidate_cached(id, revision).await?;
                after = Some((id, revision));
            }
            if rows.len() < 64 {
                return Ok(());
            }
        }
    }
    async fn refresh_cached_page(self: &Arc<Self>, after: &mut Option<(Uuid, i64)>) -> Result<()> {
        let rows=self.db.query_all(sql("SELECT track_id,revision FROM federation_cached_publications
             WHERE $1::uuid IS NULL OR (track_id,revision)>($1,$2) ORDER BY track_id,revision LIMIT 128",
             [after.map(|(id,_)|id).into(),after.map(|(_,rev)|rev).into()])).await?;
        for row in &rows {
            let id = row.try_get("", "track_id")?;
            let rev = row.try_get("", "revision")?;
            self.revalidate_cached(id, rev).await?;
            *after = Some((id, rev));
        }
        if rows.len() < 128 {
            *after = None;
            self.db
                .execute(sql(
                    "DELETE FROM federation_mirror_leases l USING (
                SELECT track_id,revision,provider FROM federation_mirror_leases
                WHERE expires_at<now()-interval '2 hours' ORDER BY expires_at LIMIT 128) old
                WHERE (l.track_id,l.revision,l.provider)=(old.track_id,old.revision,old.provider)",
                    [],
                ))
                .await?;
        }
        Ok(())
    }

    pub(super) async fn allows_range_export(
        &self,
        peer: EndpointId,
        track: Uuid,
        revision: i64,
        origin: EndpointId,
    ) -> Result<bool> {
        if !crate::peer_policy::permits_export(&self.db, &peer.to_string()).await? {
            return Ok(false);
        }
        if origin == self.node_id() {
            return Ok(self.subscribers.read().await.contains(&peer));
        }
        if !self.share_cached() {
            return Ok(false);
        }
        let row=self.db.query_one(sql("SELECT c.content_hash FROM federation_mirror_leases l JOIN federation_cached_publications c USING(track_id,revision)
                WHERE l.track_id=$1 AND l.revision=$2 AND l.provider=$3 AND l.active AND l.expires_at>now()",
            [track.into(),revision.into(),self.node_id().to_string().into()])).await?;
        let Some(row) = row else { return Ok(false) };
        Ok(self.cache_contains(row.try_get::<String>("", "content_hash")?.parse()?))
    }
}
