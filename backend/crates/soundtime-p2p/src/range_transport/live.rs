//! Admission, provider selection and in-flight sharing for publication-scoped windows.
#[path = "budget.rs"]
mod budget;
use super::{ConnectionPool, P2pError, P2pNode};
use crate::range_transport::{self, WindowRequest};
use anyhow::{anyhow, bail, ensure, Context, Result};
use bytes::Bytes;
use futures_util::future::{BoxFuture, FutureExt, Shared, WeakShared};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointId};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use soundtime_db::media_access::ReadGuard;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

const MAX_PROVIDERS: i64 = 12;
const MAX_QUEUED_WINDOWS: usize = 512;

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(8);

type Transfer = BoxFuture<'static, Result<Bytes, Arc<str>>>;

enum SourceRead<T> {
    Verified(T),
    Unavailable,
    LostSession,
}

// One fresh handshake at most per window across the entire provider list.
// Every other eligible source is tried before reattempting a lost session.
async fn read_from_sources<T>(
    peers: Vec<EndpointId>,
    mut read: impl AsyncFnMut(EndpointId) -> Result<SourceRead<T>>,
) -> Result<Option<T>> {
    let mut remaining = VecDeque::from(peers);
    let mut redial_scheduled = false;
    while let Some(peer) = remaining.pop_front() {
        match read(peer).await? {
            SourceRead::Verified(bytes) => return Ok(Some(bytes)),
            SourceRead::LostSession if !redial_scheduled => {
                remaining.push_back(peer);
                redial_scheduled = true;
            }
            _ => {}
        }
    }
    Ok(None)
}

async fn read_remote_window(
    pool: &ConnectionPool,
    peer: EndpointId,
    req: &WindowRequest,
    track: &ReadGuard,
    relay_only: bool,
    timeout: Duration,
) -> Result<SourceRead<(range_transport::VerifiedWindow, bool)>> {
    let deadline = tokio::time::Instant::now() + timeout;
    let connection = match tokio::time::timeout_at(deadline, pool.get_connection(peer)).await {
        Ok(Err(P2pError::ConnectionPoolBusy)) => return Ok(SourceRead::Unavailable),
        result => result.context("range handshake timed out")??,
    };
    let relay_only_path = || {
        let paths = connection.paths();
        paths
            .iter()
            .any(|path| path.is_selected() && path.is_relay())
            && paths.iter().all(|path| path.is_relay())
    };
    let before = relay_only_path();
    let fetch = range_transport::fetch_window(&connection, req, track.subscription());
    match tokio::time::timeout_at(deadline, fetch).await {
        Ok(Ok(window)) => Ok(SourceRead::Verified((
            window,
            (before || relay_only) && relay_only_path(),
        ))),
        Ok(Err(error))
            if connection.close_reason().is_none()
                && !error
                    .chain()
                    .any(|cause| cause.is::<iroh::endpoint::ConnectionError>()) =>
        {
            // A stream reset, revoked grant or invalid Bao proof is not evidence
            // of a dead connection. Only a connection-level error merits a redial.
            Err(error)
        }
        _ => {
            pool.invalidate_connection(&connection);
            Ok(SourceRead::LostSession)
        }
    }
}

pub(super) struct RangeTransfers {
    pub(crate) pool: ConnectionPool,
    per_peer: usize,
    upload: budget::Budget,
    download: budget::Budget,
    reads: Arc<Semaphore>,
    writes: Arc<Semaphore>,
    readers_by_peer: Mutex<HashMap<EndpointId, Weak<Semaphore>>>,
    writers_by_peer: Mutex<HashMap<EndpointId, Weak<Semaphore>>>,
    in_flight: Mutex<HashMap<WindowRequest, WeakShared<Transfer>>>,
    failed: Mutex<HashMap<EndpointId, Instant>>,
    pub(super) proof_bytes_received: AtomicU64,
    pub(super) verified_windows: AtomicU64,
    pub(super) relay_windows_verified: AtomicU64,
}

impl RangeTransfers {
    pub(super) fn new(endpoint: Endpoint) -> Self {
        let active = budget::setting("P2P_MAX_RANGE_TRANSFERS", 32, 2048).max(1) as usize;
        Self {
            per_peer: budget::setting("P2P_MAX_RANGES_PER_PEER", 4, 256).max(1) as usize,
            upload: budget::Budget::new(
                "P2P_UPLOAD_BYTES_PER_SECOND",
                "P2P_PEER_UPLOAD_BYTES_PER_SECOND",
            ),
            download: budget::Budget::new(
                "P2P_DOWNLOAD_BYTES_PER_SECOND",
                "P2P_PEER_DOWNLOAD_BYTES_PER_SECOND",
            ),
            pool: ConnectionPool::new(endpoint, range_transport::ALPN),
            reads: Arc::new(Semaphore::new(active)),
            writes: Arc::new(Semaphore::new(active)),
            readers_by_peer: Mutex::default(),
            writers_by_peer: Mutex::default(),
            in_flight: Mutex::default(),
            failed: Mutex::default(),
            proof_bytes_received: AtomicU64::new(0),
            verified_windows: AtomicU64::new(0),
            relay_windows_verified: AtomicU64::new(0),
        }
    }

    fn flight(
        &self,
        req: WindowRequest,
        start: impl FnOnce() -> Transfer,
    ) -> Result<Shared<Transfer>> {
        let mut active = self.in_flight.lock().expect("range transfers");
        if let Some(existing) = active.get(&req).and_then(WeakShared::upgrade) {
            return Ok(existing);
        }
        active.retain(|_, entry| entry.upgrade().is_some());
        ensure!(active.len() < MAX_QUEUED_WINDOWS, "too many range requests");
        let fetch = start().shared();
        active.insert(req, fetch.downgrade().expect("new shared transfer"));
        Ok(fetch)
    }

    fn peer_slots(
        &self,
        peers: &Mutex<HashMap<EndpointId, Weak<Semaphore>>>,
        peer: EndpointId,
    ) -> Arc<Semaphore> {
        let mut peers = peers.lock().expect("range peer slots");
        if let Some(slots) = peers.get(&peer).and_then(Weak::upgrade) {
            return slots;
        }
        peers.retain(|_, sem| sem.strong_count() > 0);
        let slots = Arc::new(Semaphore::new(self.per_peer));
        peers.insert(peer, Arc::downgrade(&slots));
        slots
    }
}

impl P2pNode {
    pub fn range_streaming(&self) -> bool {
        self._config.range_streaming
    }

    /// None denotes an unattributed legacy row. A known v2 publication never falls back to a raw hash.
    pub async fn range_manifest(&self, id: Uuid) -> Result<Option<WindowRequest>> {
        let row = self.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT c.node_id,p.origin_publication_id,p.revision,p.state,v.content_hash,v.file_size
             FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
             LEFT JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
             WHERE p.track_id=$1", [id.into()])).await?;
        let Some(row) = row else { return Ok(None) };
        ensure!(
            row.try_get::<String>("", "state")? == "active",
            "inactive publication"
        );
        let hash: String = row.try_get("", "content_hash")?;
        Ok(Some(WindowRequest {
            origin: row.try_get::<String>("", "node_id")?.parse()?,
            publication: row.try_get("", "origin_publication_id")?,
            revision: u64::try_from(row.try_get::<i64>("", "revision")?)?,
            hash: hash.parse()?,
            file_size: u64::try_from(row.try_get::<i64>("", "file_size")?)?,
            offset: 0,
            length: 0,
        }))
    }

    /// This is resolved at each end of a window. Only a fresh offer for this exact
    /// publication/version may supply it. Keeping a hash in the store grants no access.
    async fn providers(&self, id: Uuid, req: &WindowRequest) -> Result<Vec<EndpointId>> {
        let rows = self.db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT DISTINCT s.provider FROM federation_track_providers s
             JOIN track_publications p ON p.track_id=s.track_id
             JOIN federation_catalogs c ON c.id=p.catalog_id
             JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
             WHERE p.track_id=$1 AND p.state='active' AND c.node_id=$2 AND p.origin_publication_id=$3
               AND p.revision=$4 AND v.content_hash=$5 AND v.file_size=$6
               AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
               AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=v.content_hash)
               AND s.allowed AND (NOT $7 OR s.followed) AND s.online ORDER BY s.provider LIMIT $8",
            [id.into(),req.origin.to_string().into(),req.publication.into(),i64::try_from(req.revision)?.into(),
             req.hash.to_string().into(),i64::try_from(req.file_size)?.into(),self.explicit_catalog().into(),MAX_PROVIDERS.into()])).await?;
        let mut providers = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("", "provider")?;
            if let Ok(id) = id.parse() {
                if id != self.node_id() || self.cache_contains(req.hash) {
                    providers.push(id);
                }
            }
        }
        let mut failures = self.ranges.failed.lock().expect("range supplier cooldown");
        failures.retain(|_, at| at.elapsed() < Duration::from_secs(10));
        providers.sort_by_key(|id| {
            (
                failures.contains_key(id),
                *id != self.node_id(),
                *id != req.origin,
            )
        });
        Ok(providers)
    }

    pub async fn can_retain_version(&self, id: Uuid, req: &WindowRequest) -> Result<bool> {
        Ok(!self.providers(id, req).await?.is_empty())
    }

    pub async fn verified_range(self: &Arc<Self>, id: Uuid, req: WindowRequest) -> Result<Bytes> {
        req.range()?;
        let transfer = self.ranges.flight(req.clone(), || {
            let node = Arc::clone(self);
            async move {
                node.fetch_from_providers(id, &req)
                    .await
                    .map_err(|e| Arc::<str>::from(e.to_string()))
            }
            .boxed()
        })?;
        // Only reader futures own this transfer. Cancelling the last HTTP reader
        // drops the QUIC receive stream even while it waits for a provider.
        transfer.await.map_err(|e| anyhow!("{e}"))
    }

    async fn fetch_from_providers(&self, id: Uuid, req: &WindowRequest) -> Result<Bytes> {
        let track = ReadGuard::track(id);
        let providers = self.providers(id, req).await?;
        ensure!(!providers.is_empty(), "no authorized range provider");
        read_from_sources(providers, async |peer| {
            let peer_grant = ReadGuard::peer(&peer.to_string());
            // Acquire a new supplier grant on retry. The original track grant
            // and immutable request persist across every attempt.
            let attempt = self.fetch_from_provider(id, req, peer, &track);
            match track.during(peer_grant.during(attempt)).await? {
                Ok(fetched) => fetched,
                Err(_) => Ok(SourceRead::Unavailable),
            }
        })
        .await?
        .with_context(|| {
            format!(
                "range providers unavailable for publication {}",
                req.publication
            )
        })
    }

    async fn fetch_from_provider(
        &self,
        id: Uuid,
        req: &WindowRequest,
        peer: EndpointId,
        track: &ReadGuard,
    ) -> Result<SourceRead<Bytes>> {
        // The bounded flight table also covers these local waits. A slow local
        // quota must not use the peer's network timeout or record a failed peer.
        let slots = self.ranges.peer_slots(&self.ranges.readers_by_peer, peer);
        let _peer = slots.acquire_owned().await?;
        let _global = self.ranges.reads.acquire().await?;
        let remote = peer != self.node_id();
        if remote {
            self.ranges
                .download
                .admit(peer, req.length as usize)
                .await?;
        }
        if !self.providers(id, req).await?.contains(&peer) {
            return Ok(SourceRead::Unavailable);
        }

        let (window, relayed) = if remote {
            match read_remote_window(
                &self.ranges.pool,
                peer,
                req,
                track,
                self._config.relay_only,
                ATTEMPT_TIMEOUT,
            )
            .await
            {
                Ok(SourceRead::Verified(window)) => window,
                Ok(SourceRead::LostSession) => {
                    self.note_range_failure(peer);
                    return Ok(SourceRead::LostSession);
                }
                Ok(SourceRead::Unavailable) => return Ok(SourceRead::Unavailable),
                Err(error) => {
                    tracing::debug!(%peer,%error,"range provider did not supply a verified window");
                    self.note_range_failure(peer);
                    return Ok(SourceRead::Unavailable);
                }
            }
        } else {
            let encoded = match self.encode_range(req).await {
                Ok(encoded) => encoded,
                Err(_) => return Ok(SourceRead::Unavailable),
            };
            let Ok(window) = range_transport::decode_window(req, &encoded) else {
                return Ok(SourceRead::Unavailable);
            };
            (window, false)
        };
        if !self.providers(id, req).await?.contains(&peer) {
            return Ok(SourceRead::Unavailable);
        }
        if remote {
            self.ranges.proof_bytes_received.fetch_add(
                (window.response_bytes - window.data.len()) as u64,
                Ordering::Relaxed,
            );
            self.ranges.verified_windows.fetch_add(1, Ordering::Relaxed);
            if relayed {
                self.ranges
                    .relay_windows_verified
                    .fetch_add(1, Ordering::Relaxed);
            }
            self.audio_bytes_received
                .fetch_add(window.data.len() as u64, Ordering::Relaxed);
        }
        Ok(SourceRead::Verified(window.data))
    }

    fn note_range_failure(&self, peer: EndpointId) {
        let mut failures = self.ranges.failed.lock().expect("range supplier cooldown");
        if failures.len() < MAX_QUEUED_WINDOWS {
            failures.insert(peer, Instant::now());
        }
    }

    pub(super) async fn handle_range_connection(
        self: &Arc<Self>,
        conn: Connection,
    ) -> Result<(), P2pError> {
        let peer = conn.remote_id();
        let connection_grant = ReadGuard::peer(&peer.to_string());
        if !self.range_streaming()
            || crate::peer_policy::get(&self.db, &peer.to_string())
                .await?
                .banned
        {
            conn.close(1u32.into(), b"range transfer unavailable");
            return Ok(());
        }
        let per_peer = self.ranges.peer_slots(&self.ranges.writers_by_peer, peer);
        while let Ok(Ok((mut send, mut recv))) = connection_grant.during(conn.accept_bi()).await {
            let permits = (
                per_peer.clone().try_acquire_owned(),
                self.ranges.writes.clone().try_acquire_owned(),
            );
            let (Ok(remote), Ok(global)) = permits else {
                let _ = send.reset(2u32.into());
                let _ = recv.stop(2u32.into());
                continue;
            };
            let node = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(error) = node.serve_range(peer, send, recv, (remote, global)).await {
                    tracing::debug!(%peer, %error, "range stream closed");
                }
            });
        }
        conn.close(0u32.into(), b"range session ended");
        Ok(())
    }

    async fn encode_range(&self, req: &WindowRequest) -> Result<Vec<u8>> {
        if let Some(bytes) = self.blob_cache.encode(req).await? {
            return Ok(bytes);
        }
        range_transport::encode_window(&self.blob_store, req.clone()).await
    }

    async fn local_publication(&self, req: &WindowRequest) -> Result<Uuid> {
        let row = self.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT p.track_id FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
             JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
             WHERE p.state='active' AND c.node_id=$1 AND p.origin_publication_id=$2 AND p.revision=$3
               AND v.content_hash=$4 AND v.file_size=$5
               AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
               AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=v.content_hash)
               AND ((c.is_local AND NOT EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=p.track_id))
                 OR ($6 AND EXISTS(SELECT FROM federation_cached_publications cp WHERE cp.track_id=p.track_id
                       AND cp.revision=p.revision AND cp.content_hash=v.content_hash)))",
            [req.origin.to_string().into(),req.publication.into(),i64::try_from(req.revision)?.into(),
             req.hash.to_string().into(), i64::try_from(req.file_size)?.into(),self.cache_contains(req.hash).into()])).await?;
        Ok(row.context("publication denied")?.try_get("", "track_id")?)
    }

    async fn serve_range(
        &self,
        peer: EndpointId,
        mut send: SendStream,
        mut recv: RecvStream,
        _permits: (OwnedSemaphorePermit, OwnedSemaphorePermit),
    ) -> Result<()> {
        let peer_grant = ReadGuard::peer(&peer.to_string());
        let result = peer_grant.during(async {
            let req = tokio::time::timeout(Duration::from_secs(15), range_transport::read_request(&mut recv)).await??;
            let id = self.local_publication(&req).await?;
            let track = ReadGuard::track(id);
            let stopped = send.stopped();
            tokio::select! {
                biased;
                _ = stopped => bail!("reader disconnected"),
                result = track.during(async {
                    ensure!(!crate::peer_policy::get(&self.db,&peer.to_string()).await?.banned, "peer banned");
                    ensure!(self.allows_range_export(peer,id,i64::try_from(req.revision)?,req.origin).await?, "publication export denied");
                    let bytes = self.encode_range(&req).await?;
                    self.local_publication(&req).await?;
                    ensure!(self.allows_range_export(peer,id,i64::try_from(req.revision)?,req.origin).await?, "publication export revoked");
                    self.ranges.upload.admit(peer, bytes.len()).await?;
                    self.local_publication(&req).await?;
                    ensure!(self.allows_range_export(peer,id,i64::try_from(req.revision)?,req.origin).await?, "publication export revoked during admission");
                    tokio::time::timeout(Duration::from_secs(15), send.write_all(&bytes)).await??;
                    send.finish()?;
                    self.audio_bytes_sent.fetch_add(u64::from(req.length), Ordering::Relaxed);
                    Ok::<_,anyhow::Error>(())
                }) => result?,
            }
        }).await;
        if !matches!(result, Ok(Ok(()))) {
            let _ = send.reset(1u32.into());
        }
        result??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::join_all;
    use iroh::endpoint::presets::Minimal;
    use iroh::EndpointAddr;
    use iroh_blobs::store::mem::MemStore;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::watch;

    async fn loopback() -> Result<Endpoint> {
        Ok(Endpoint::builder(Minimal)
            .clear_ip_transports()
            .clear_relay_transports()
            .portmapper_config(iroh::endpoint::PortmapperConfig::Disabled)
            .bind_addr("127.0.0.1:0")?
            .alpns(vec![range_transport::ALPN.to_vec()])
            .bind()
            .await?)
    }

    #[tokio::test]
    async fn tries_other_sources_before_a_single_redial() -> Result<()> {
        let origin = iroh::SecretKey::generate().public();
        let mirror = iroh::SecretKey::generate().public();
        let mut attempts = Vec::new();
        let result = read_from_sources(vec![origin, mirror], async |peer| {
            attempts.push(peer);
            Ok(SourceRead::<Bytes>::LostSession)
        })
        .await?;
        assert!(result.is_none());
        assert_eq!(attempts, [origin, mirror, origin]);
        Ok(())
    }

    #[tokio::test]
    async fn a_window_recovers_after_the_only_publisher_restarts() -> Result<()> {
        use iroh::address_lookup::memory::MemoryLookup;

        tokio::time::timeout(Duration::from_secs(20), async {
            let old = loopback().await?;
            let lookup = MemoryLookup::new();
            let base = || {
                Endpoint::builder(Minimal)
                    .clear_ip_transports()
                    .clear_relay_transports()
                    .portmapper_config(iroh::endpoint::PortmapperConfig::Disabled)
                    .alpns(vec![range_transport::ALPN.to_vec()])
                    .bind_addr("127.0.0.1:0")
            };
            let new = base()?.secret_key(old.secret_key().clone()).bind().await?;
            assert_eq!(new.id(), old.id());
            let reader = base()?.address_lookup(lookup.clone()).bind().await?;
            let pool = ConnectionPool::new(reader, range_transport::ALPN);
            lookup.set_endpoint_info(
                EndpointAddr::new(old.id()).with_ip_addr(old.bound_sockets()[0]),
            );
            let (original, incoming) = tokio::join!(pool.get_connection(old.id()), async {
                old.accept()
                    .await
                    .context("first endpoint closed")?
                    .await
                    .map_err(anyhow::Error::from)
            });
            let (original, incoming) = (original?, incoming?);
            let store = MemStore::new();
            let data = vec![9; 200_000];
            let req = WindowRequest {
                origin: old.id(),
                publication: Uuid::new_v4(),
                revision: 2,
                hash: store.blobs().add_slice(&data).await?.hash,
                file_size: data.len() as u64,
                offset: 65536,
                length: range_transport::WINDOW_BYTES,
            };
            let track = ReadGuard::track(Uuid::new_v4());
            let (_authority, grant) = watch::channel(true);
            let (first, served) = tokio::join!(
                read_remote_window(&pool, old.id(), &req, &track, false, ATTEMPT_TIMEOUT),
                range_transport::serve_window(&incoming, &store, |_, _| Some(grant.clone()))
            );
            served?;
            assert!(matches!(first?, SourceRead::Verified(_)));

            // Stop serving the original QUIC session without notifying the client.
            // After the first attempt, rediscovery supplies a fresh endpoint with
            // the same cryptographic identity and the same immutable publication.
            let mut calls = 0;
            let recovery = read_from_sources(vec![old.id()], async |peer| {
                calls += 1;
                let timeout = if calls == 1 {
                    Duration::from_millis(100)
                } else {
                    ATTEMPT_TIMEOUT
                };
                let fetched = read_remote_window(&pool, peer, &req, &track, false, timeout).await?;
                if matches!(&fetched, SourceRead::LostSession) {
                    old.close().await;
                    lookup.set_endpoint_info(
                        EndpointAddr::new(new.id()).with_ip_addr(new.bound_sockets()[0]),
                    );
                }
                Ok(fetched)
            });
            let serving = async {
                let connection = new
                    .accept()
                    .await
                    .context("restarted endpoint closed")?
                    .await?;
                range_transport::serve_window(&connection, &store, |_, _| Some(grant.clone()))
                    .await?;
                Ok::<_, anyhow::Error>(connection)
            };
            let (recovered, new_connection) = tokio::join!(recovery, serving);
            let _new_connection = new_connection?;
            let (window, _) = recovered?.context("no verified window after restart")?;
            assert_eq!(calls, 2);
            assert_eq!(window.data.as_ref(), &data[65536..131072]);
            assert_eq!(pool.established(), 2);
            // A late error from the original cannot remove the new connection.
            pool.invalidate_connection(&original);
            assert_eq!(pool.len().await, 1);
            Ok::<_, anyhow::Error>(())
        })
        .await?
    }

    #[tokio::test]
    async fn simultaneous_readers_share_one_quic_stream_and_cancellation_drops_the_last_owner(
    ) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(20), async {
            let server = loopback().await?;
            let client = loopback().await?;
            let addr = EndpointAddr::new(server.id()).with_ip_addr(server.bound_sockets()[0]);
            let (outgoing, incoming) =
                tokio::join!(client.connect(addr, range_transport::ALPN), async {
                    server
                        .accept()
                        .await
                        .context("server closed")?
                        .await
                        .map_err(anyhow::Error::from)
                });
            let (outgoing, incoming) = (outgoing?, incoming?);
            let manager = RangeTransfers::new(client);
            let store = MemStore::new();
            let content = vec![37; 500_000];
            let hash = store.blobs().add_slice(&content).await?.hash;
            let req = WindowRequest {
                origin: server.id(),
                publication: Uuid::new_v4(),
                revision: 5,
                hash,
                file_size: content.len() as u64,
                offset: 131_072,
                length: range_transport::WINDOW_BYTES,
            };
            let starts = AtomicUsize::new(0);
            let (_issuer, grant) = watch::channel(true);
            let mut readers = Vec::new();
            for _ in 0..24 {
                readers.push(manager.flight(req.clone(), || {
                    starts.fetch_add(1, Ordering::Relaxed);
                    let (conn, req, grant) = (outgoing.clone(), req.clone(), grant.clone());
                    async move {
                        range_transport::fetch_window(&conn, &req, grant)
                            .await
                            .map(|win| win.data)
                            .map_err(|e| Arc::<str>::from(e.to_string()))
                    }
                    .boxed()
                })?);
            }
            let (responses, served) = tokio::join!(
                join_all(readers),
                range_transport::serve_window(&incoming, &store, |_, _| Some(grant.clone()))
            );
            served?;
            assert_eq!(starts.load(Ordering::Relaxed), 1);
            for response in responses {
                assert_eq!(response.unwrap().as_ref(), &content[131_072..196_608]);
            }
            let (sentinel, release) = tokio::sync::oneshot::channel::<()>();
            let last = manager.flight(req, || {
                async move {
                    let _captured = sentinel;
                    std::future::pending().await
                }
                .boxed()
            })?;
            let other = last.clone();
            drop(other);
            drop(last);
            assert!(
                release.await.is_err(),
                "transfer retained after all readers cancelled"
            );
            Ok::<_, anyhow::Error>(())
        })
        .await?
    }
}
