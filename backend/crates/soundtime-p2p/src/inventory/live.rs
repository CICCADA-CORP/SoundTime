//! Negotiated, pull-based catalogue RPC. No audio or cover store is accessible here.
use super::{ConnectionPool, P2pMessage, P2pNode};
use crate::inventory::{
    self,
    source::{self, Request, Response},
    Page,
};
use anyhow::{bail, ensure, Context, Result};
use iroh::{endpoint::Connection, Endpoint, EndpointId};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{atomic::Ordering, Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{RwLock, Semaphore};
use uuid::Uuid;

const FRAME: usize = inventory::PAGE_BYTES + 4096;
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
const PAGE_BUDGET: usize = 16;
pub(super) struct Catalogs {
    pool: ConnectionPool,
    capable: RwLock<HashSet<EndpointId>>,
    active: Mutex<HashMap<EndpointId, Weak<tokio::sync::Mutex<()>>>>,
    reads: Arc<Semaphore>,
    writes: Semaphore,
    withdrawals: Arc<Semaphore>,
    notices: Arc<Semaphore>,
}
impl Catalogs {
    pub(super) fn new(endpoint: Endpoint) -> Self {
        Self {
            pool: ConnectionPool::new(endpoint, inventory::ALPN),
            capable: Default::default(),
            active: Default::default(),
            reads: Arc::new(Semaphore::new(8)),
            writes: Semaphore::new(16),
            withdrawals: Arc::new(Semaphore::new(2)),
            notices: Arc::new(Semaphore::new(4)),
        }
    }
    pub(super) fn notice_slot(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, tokio::sync::TryAcquireError> {
        self.notices.clone().try_acquire_owned()
    }
    pub(super) async fn capability(&self, id: EndpointId, capable: bool) {
        let mut peers = self.capable.write().await;
        if capable {
            peers.insert(id);
        } else {
            peers.remove(&id);
        }
    }
    pub(super) async fn supports(&self, id: EndpointId) -> bool {
        self.capable.read().await.contains(&id)
    }
    fn peer_lock(&self, id: EndpointId) -> Arc<tokio::sync::Mutex<()>> {
        let mut active = self.active.lock().expect("catalogue jobs");
        active.retain(|_, job| job.strong_count() > 0);
        if let Some(job) = active.get(&id).and_then(Weak::upgrade) {
            return job;
        }
        let job = Arc::new(tokio::sync::Mutex::new(()));
        active.insert(id, Arc::downgrade(&job));
        job
    }
}

/// All catalogue requests read a fixed position or a summary. Repeating Head
/// registers the same reader idempotently. Apply a response only once, outside
/// this exchange, after the final policy check.
async fn exchange_catalog<F: Future<Output = Result<()>>>(
    pool: &ConnectionPool,
    peer: EndpointId,
    request: &Request,
    authorize: impl Fn() -> F,
) -> Result<Response> {
    use iroh::endpoint::{ConnectionError, ReadToEndError};
    use tokio::time::{timeout_at, Instant};

    let bytes = serde_json::to_vec(request)?;
    let deadline = Instant::now() + RPC_TIMEOUT * 2;
    let mut retried = false;
    loop {
        timeout_at(deadline, authorize())
            .await
            .context("catalogue policy timed out")??;
        let attempt = deadline.min(Instant::now() + RPC_TIMEOUT);
        let conn = timeout_at(attempt, pool.get_connection(peer))
            .await
            .context("catalogue dial timed out")??;
        // A small Head is how every resumption starts. Leave time to replace
        // a pre-crash QUIC connection that still accepts streams locally.
        let until = if !retried && matches!(request, Request::Head) {
            attempt.min(Instant::now() + Duration::from_secs(3))
        } else {
            attempt
        };
        let operation = async {
            let (mut send, mut recv) = conn.open_bi().await?;
            send.write_all(&bytes).await?;
            send.finish()?;
            Ok::<_, anyhow::Error>(recv.read_to_end(FRAME).await?)
        };
        let failure = match timeout_at(until, operation).await {
            Ok(Ok(reply)) => {
                timeout_at(deadline, authorize())
                    .await
                    .context("catalogue policy timed out")??;
                return match serde_json::from_slice(&reply)? {
                    Response::Preparing => Err(crate::P2pError::CatalogPreparing.into()),
                    response => Ok(response),
                };
            }
            Ok(Err(error)) => {
                let oversized = matches!(
                    error.downcast_ref::<ReadToEndError>(),
                    Some(ReadToEndError::TooLong)
                );
                let lost = conn.close_reason().is_some()
                    || error.chain().any(|cause| cause.is::<ConnectionError>());
                if oversized || !lost {
                    return Err(error);
                }
                error
            }
            Err(timeout) => anyhow::Error::new(timeout).context("catalogue RPC timed out"),
        };
        // Compare the stable QUIC ID: late failures cannot evict a replacement
        // installed by another reader of the same catalogue.
        pool.invalidate_connection(&conn);
        if retried || Instant::now() >= deadline {
            return Err(failure);
        }
        tracing::debug!(%peer, %failure, "retrying catalogue read on a fresh connection");
        retried = true;
    }
}

impl P2pNode {
    pub fn versioned_catalog(&self) -> bool {
        self._config.versioned_catalog && self.explicit_catalog()
    }

    pub(super) async fn catalog_rpc(&self, peer: EndpointId, request: Request) -> Result<Response> {
        let id = peer.to_string();
        exchange_catalog(&self.catalogs.pool, peer, &request, || async {
            let policy = crate::peer_policy::get(&self.db, &id).await?;
            if policy.banned {
                return Err(crate::P2pError::PeerBlocked(id.clone()).into());
            }
            ensure!(
                policy.followed || matches!(request, Request::Withdrawal { .. }),
                "catalogue provider is no longer followed"
            );
            Ok(())
        })
        .await
    }

    pub(super) async fn serve_catalog(&self, conn: Connection) -> Result<()> {
        ensure!(self.versioned_catalog(), "catalogue protocol disabled");
        let peer = conn.remote_id();
        while let Ok(Ok((mut send, mut recv))) =
            tokio::time::timeout(Duration::from_secs(60), conn.accept_bi()).await
        {
            let _permit = self
                .catalogs
                .writes
                .try_acquire()
                .context("catalogue server at capacity")?;
            tokio::time::timeout(RPC_TIMEOUT, async {
                ensure!(
                    crate::peer_policy::permits_export(&self.db, &peer.to_string()).await?,
                    "catalogue reader blocked"
                );
                let request: Request = serde_json::from_slice(&recv.read_to_end(512).await?)?;
                let subscribing = matches!(request, Request::Head);
                let response =
                    source::answer(&self.db, self.endpoint.secret_key(), request).await?;
                let bytes = serde_json::to_vec(&response)?;
                ensure!(
                    bytes.len() <= FRAME,
                    "catalogue response exceeds frame bound"
                );
                ensure!(
                    crate::peer_policy::permits_export(&self.db, &peer.to_string()).await?,
                    "catalogue reader blocked"
                );
                if subscribing {
                    self.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                        "INSERT INTO federation_catalog_clients(node_id) VALUES($1) ON CONFLICT DO NOTHING",
                        [peer.to_string().into()])).await?;
                }
                send.write_all(&bytes).await?;
                send.finish()?;
                Ok::<_, anyhow::Error>(())
            })
            .await??;
        }
        Ok(())
    }

    pub(super) async fn schedule_catalog_pull(self: &Arc<Self>, peer: EndpointId) {
        let lock = self.catalogs.peer_lock(peer);
        let Ok(guard) = lock.try_lock_owned() else {
            return;
        };
        let Ok(permit) = self.catalogs.reads.clone().try_acquire_owned() else {
            return;
        };
        let node = Arc::clone(self);
        tokio::spawn(async move {
            let (_permit, _guard) = (permit, guard);
            if let Err(error) = node.pull_pages(peer).await {
                tracing::debug!(%peer,%error,"catalogue sync deferred");
            }
        });
    }

    pub(super) async fn pull_catalog(&self, peer: EndpointId) -> Result<()> {
        let lock = self.catalogs.peer_lock(peer);
        let _guard = lock.lock_owned().await;
        let _permit = self.catalogs.reads.acquire().await?;
        self.pull_pages(peer).await
    }

    async fn pull_pages(&self, peer: EndpointId) -> Result<()> {
        let Response::Head(head) = self.catalog_rpc(peer, Request::Head).await? else {
            bail!("missing catalogue summary")
        };
        ensure!(head >= 0, "invalid catalogue summary");
        let row=self.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT p.applied_cursor,i.snapshot,i.cut,i.complete,i.next_page,i.last_publication,
                    i.started_at<now()-interval '6 hours' AS refresh
             FROM federation_peer_policies p LEFT JOIN federation_inventories i ON i.provider=p.node_id
             WHERE p.node_id=$1 AND p.is_followed AND NOT p.is_banned",[peer.to_string().into()])).await?.context("subscription ended")?;
        let previous: Option<Uuid> = row.try_get("", "snapshot")?;
        let complete = row
            .try_get::<Option<bool>>("", "complete")?
            .unwrap_or(false);
        let renew = row.try_get::<Option<bool>>("", "refresh")?.unwrap_or(false);
        let mut cursor: i64 = row.try_get("", "applied_cursor")?;
        ensure!(cursor <= head, "remote journal moved backwards");
        if complete && !renew {
            for _ in 0..PAGE_BUDGET {
                if cursor == head {
                    break;
                }
                let Response::Changes(changes) = self
                    .catalog_rpc(
                        peer,
                        Request::Changes {
                            cut: head,
                            after: cursor,
                        },
                    )
                    .await?
                else {
                    bail!("expected changes")
                };
                ensure!(
                    !changes.is_empty() && changes.len() <= inventory::PAGE_ENTRIES,
                    "missing or oversized change batch"
                );
                self.catalog_entries_received
                    .fetch_add(changes.len() as u64, Ordering::Relaxed);
                for change in changes {
                    ensure!(
                        cursor.checked_add(1) == Some(change.cursor) && change.cursor <= head,
                        "noncontiguous changes"
                    );
                    inventory::apply_delta(&self.db, peer, &serde_json::to_vec(&change)?).await?;
                    cursor = change.cursor;
                }
            }
            return self.pull_mirrors(peer).await;
        }
        let (snapshot, cut, mut number, mut after) = match previous.filter(|_| !complete) {
            Some(snap) => (
                snap,
                row.try_get("", "cut")?,
                row.try_get("", "next_page")?,
                row.try_get("", "last_publication")?,
            ),
            None => {
                let snap = Uuid::new_v4();
                inventory::begin(&self.db, peer, snap, head).await?;
                (snap, head, 0, None)
            }
        };
        for _ in 0..PAGE_BUDGET {
            let Response::Snapshot { entries, done } = self
                .catalog_rpc(peer, Request::Snapshot { cut, after })
                .await?
            else {
                bail!("expected snapshot page")
            };
            ensure!(
                entries.len() <= inventory::PAGE_ENTRIES && (done || !entries.is_empty()),
                "invalid snapshot page"
            );
            self.catalog_entries_received
                .fetch_add(entries.len() as u64, Ordering::Relaxed);
            for entry in &entries {
                let id = entry.publication.manifest.publication;
                ensure!(
                    entry.publication.manifest.origin == peer
                        && after.is_none_or(|older| id > older),
                    "invalid origin catalogue order"
                );
                after = Some(id);
            }
            let page = Page {
                snapshot,
                cut,
                number,
                last: done,
                entries,
            };
            inventory::apply_page(&self.db, peer, &serde_json::to_vec(&page)?).await?;
            if done {
                break;
            }
            number += 1;
        }
        self.pull_mirrors(peer).await
    }

    pub(super) fn start_catalog_worker(self: &Arc<Self>) {
        let node = self.clone();
        let mut shutdown = node.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let prepare_local_journal = async {
                loop {
                    let ready = node
                        .db
                        .query_one(Statement::from_string(
                            DbBackend::Postgres,
                            "SELECT federation_seed_events(128) AS ready",
                        ))
                        .await;
                    match ready {
                        Ok(Some(row)) if matches!(row.try_get::<bool>("", "ready"), Ok(true)) => {
                            break
                        }
                        Err(e) => tracing::warn!(%e,"historical journal backfill deferred"),
                        _ => {}
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            };
            // Receiving remote updates and withdrawals is independent of our own
            // journal. Rotate over connected, followed sources, even during upgrade.
            let sync_subscriptions = async {
                let mut after = String::new();
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    if let Ok(batch) =
                        crate::peer_policy::followed_after(&node.db, &after, true).await
                    {
                        for id in &batch {
                            if let Ok(peer) = id.parse::<EndpointId>() {
                                if node.catalogs.supports(peer).await
                                    && node
                                        .registry
                                        .get_peer(id)
                                        .await
                                        .is_some_and(|p| p.is_online)
                                {
                                    node.schedule_catalog_pull(peer).await;
                                }
                            }
                        }
                        after = if batch.len() == 8 {
                            batch.last().cloned().unwrap_or_default()
                        } else {
                            String::new()
                        };
                    }
                }
            };
            let check_withdrawals = async {
                let mut last_retained = None;
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    if let Err(error) = node.check_retained(&mut last_retained).await {
                        tracing::debug!(%error,"withdrawal catch-up deferred");
                    }
                }
            };
            tokio::select! {_=async { tokio::join!(prepare_local_journal, sync_subscriptions, check_withdrawals); }=>{},_=shutdown.changed()=>{}}
        });
    }

    /// Check a bounded page of retained references through an origin or a known
    /// provider. Never broadcast listeners' references to unrelated peers.
    async fn check_retained(&self, after: &mut Option<Uuid>) -> Result<()> {
        let rows=self.db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT p.track_id,p.origin_publication_id,c.node_id FROM track_publications p
             JOIN federation_catalogs c ON c.id=p.catalog_id WHERE NOT c.is_local AND p.state='active'
             AND ($1::uuid IS NULL OR p.track_id>$1)
             AND EXISTS(SELECT FROM federation_withdrawal_references refs WHERE refs.track_id=p.track_id)
             ORDER BY p.track_id LIMIT 4", [(*after).into()])).await?;
        for row in &rows {
            let track: Uuid = row.try_get("", "track_id")?;
            *after = Some(track);
            let origin: EndpointId = row.try_get::<String>("", "node_id")?.parse()?;
            let publication = row.try_get("", "origin_publication_id")?;
            let others = self
                .db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT candidates.provider FROM (
                     SELECT provider_node_id AS provider FROM publication_offers WHERE track_id=$1
                     UNION SELECT provider FROM federation_mirror_leases WHERE track_id=$1
                 ) candidates JOIN federation_peer_policies p ON p.node_id=candidates.provider
                 LEFT JOIN p2p_peers peer ON peer.node_id=p.node_id
                 WHERE p.node_id<>$2 AND p.node_id<>$3 AND NOT p.is_banned
                   AND NOT EXISTS(SELECT FROM blocked_domains b WHERE b.domain=p.node_id)
                 ORDER BY (peer.is_online IS TRUE) DESC,p.is_followed DESC,p.node_id LIMIT 2",
                    [
                        track.into(),
                        origin.to_string().into(),
                        self.node_id().to_string().into(),
                    ],
                ))
                .await?;
            let mut providers = vec![origin];
            for other in others {
                if let Ok(id) = other.try_get::<String>("", "provider")?.parse() {
                    providers.push(id);
                }
            }
            for peer in providers {
                if !self.catalogs.supports(peer).await
                    || !self
                        .registry
                        .get_peer(&peer.to_string())
                        .await
                        .is_some_and(|p| p.is_online)
                {
                    continue;
                }
                if let Ok(Response::Withdrawal(Some(proof))) = self
                    .catalog_rpc(
                        peer,
                        Request::Withdrawal {
                            origin,
                            publication,
                        },
                    )
                    .await
                {
                    ensure!(
                        proof.manifest.origin == origin
                            && proof.manifest.publication == publication,
                        "unexpected withdrawal subject"
                    );
                    inventory::apply_withdrawal(&self.db, peer, &serde_json::to_vec(&proof)?)
                        .await?;
                    break;
                }
            }
        }
        if rows.len() < 4 {
            *after = None;
        }
        Ok(())
    }

    /// Deliver the authority's signed decision to known version-aware neighbours.
    /// The journal retains it across shutdown or delivery failure.
    pub async fn announce_withdrawal(self: &Arc<Self>, publication: Uuid) {
        if !self.versioned_catalog() {
            return;
        }
        let Ok(permit) = self.catalogs.withdrawals.clone().try_acquire_owned() else {
            return;
        };
        let node = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(Some(signed)) = source::withdrawal(
                &node.db,
                node.endpoint.secret_key(),
                node.node_id(),
                publication,
            )
            .await
            else {
                return;
            };
            let msg = &P2pMessage::SignedWithdrawal(signed);
            let peers = node.catalogs.capable.read().await.clone();
            use futures_util::{stream, StreamExt};
            let node = &node;
            stream::iter(peers)
                .for_each_concurrent(4, |peer| async move {
                    let known = node
                        .db
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Postgres,
                            "SELECT 1 FROM federation_catalog_clients WHERE node_id=$1",
                            [peer.to_string().into()],
                        ))
                        .await;
                    if !matches!(known, Ok(Some(_))) {
                        return;
                    }
                    if let Err(error) = node.send_message_to_peer(peer, msg).await {
                        tracing::debug!(%peer,%error,"signed withdrawal retained for catch-up");
                    }
                })
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_pool::isolated_pair;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test]
    async fn silent_catalogue_summary_redials_once_and_reuses_the_working_session() -> Result<()> {
        tokio::time::timeout(Duration::from_secs(12), async {
            let (server, pool) = isolated_pair(inventory::ALPN).await?;
            let reads = async {
                for expected in [41, 42] {
                    let result =
                        exchange_catalog(&pool, server.id(), &Request::Head, || async { Ok(()) })
                            .await?;
                    ensure!(
                        matches!(result,Response::Head(n) if n==expected),
                        "wrong catalogue head"
                    );
                }
                Ok::<_, anyhow::Error>(())
            };
            let serves = async {
                let stale = server
                    .accept()
                    .await
                    .context("missing initial connection")?
                    .await?;
                let (_silent, mut first) = stale.accept_bi().await?;
                let request = first.read_to_end(512).await?;
                ensure!(
                    matches!(serde_json::from_slice::<Request>(&request)?, Request::Head),
                    "expected summary"
                );
                let fresh = server
                    .accept()
                    .await
                    .context("missing replacement connection")?
                    .await?;
                ensure!(
                    stale.stable_id() != fresh.stable_id(),
                    "reused stale session"
                );
                for head in [41, 42] {
                    let (mut send, mut recv) = fresh.accept_bi().await?;
                    ensure!(
                        recv.read_to_end(512).await? == request,
                        "replayed a different request"
                    );
                    send.write_all(&serde_json::to_vec(&Response::Head(head))?)
                        .await?;
                    send.finish()?;
                    let _ = send.stopped().await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            tokio::try_join!(reads, serves)?;
            assert_eq!(pool.established(), 2);
            Ok(())
        })
        .await?
    }

    #[tokio::test]
    async fn catalogue_retry_and_receipt_both_enforce_current_ban() -> Result<()> {
        for interrupted in [true, false] {
            let (server, pool) = isolated_pair(inventory::ALPN).await?;
            let checks = AtomicUsize::new(0);
            let peer = server.id();
            let denied = tokio::sync::Notify::new();
            let request = exchange_catalog(&pool, peer, &Request::Head, || async {
                if checks.fetch_add(1, Ordering::Relaxed) > 0 {
                    denied.notify_one();
                    return Err(crate::P2pError::PeerBlocked(peer.to_string()).into());
                }
                Ok(())
            });
            let serve = async {
                let conn = server.accept().await.context("missing connection")?.await?;
                let (mut send, mut recv) = conn.accept_bi().await?;
                recv.read_to_end(512).await?;
                if interrupted {
                    // Keep the old session alive and silent until the retry's policy check.
                    denied.notified().await;
                } else {
                    send.write_all(&serde_json::to_vec(&Response::Head(19))?)
                        .await?;
                    send.finish()?;
                    let _ = send.stopped().await?;
                }
                Ok::<_, anyhow::Error>(())
            };
            let (result, served) = tokio::time::timeout(Duration::from_secs(12), async {
                tokio::join!(request, serve)
            })
            .await?;
            served?;
            assert!(matches!(
                result.unwrap_err().downcast_ref::<crate::P2pError>(),
                Some(crate::P2pError::PeerBlocked(_))
            ));
            assert_eq!(checks.load(Ordering::Relaxed), 2);
            assert_eq!(pool.established(), 1);
        }
        Ok(())
    }
}
