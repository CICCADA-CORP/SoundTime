//! Decisions at the byte-serving boundary and cancellation of in-process readers.
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};
use std::{
    collections::HashMap,
    future::Future,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock, Mutex,
    },
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Hash, PartialEq, Eq)]
enum Key {
    Track(Uuid),
    LegacyHash(String),
    Peer(String),
}

type Active = HashMap<Key, (watch::Sender<bool>, usize)>;
static READERS: LazyLock<Mutex<Active>> = LazyLock::new(Mutex::default);

static POLICY_CONNECTED: AtomicBool = AtomicBool::new(true);

/// Register before the database lookup so a withdrawal committed during admission
/// either denies the lookup or wakes the reader. Idle tracks occupy no entries.
pub struct ReadGuard {
    key: Key,
    receiver: watch::Receiver<bool>,
}

impl ReadGuard {
    fn new(key: Key) -> Self {
        let mut readers = READERS.lock().expect("media reader lock");
        let (sender, users) = readers.entry(key.clone()).or_insert_with(|| {
            (
                watch::channel(POLICY_CONNECTED.load(Ordering::Acquire)).0,
                0,
            )
        });
        *users += 1;
        Self {
            key,
            receiver: sender.subscribe(),
        }
    }

    pub fn track(id: Uuid) -> Self {
        Self::new(Key::Track(id))
    }

    pub fn peer(id: &str) -> Self {
        Self::new(Key::Peer(id.to_owned()))
    }

    pub fn subscription(&self) -> watch::Receiver<bool> {
        self.receiver.clone()
    }

    pub fn legacy_hash(hash: &str) -> Self {
        Self::new(Key::LegacyHash(hash.to_owned()))
    }

    pub async fn during<T>(&self, operation: impl Future<Output = T>) -> io::Result<T> {
        let mut state = self.receiver.clone();
        tokio::select! {
            biased;
            _ = state.wait_for(|allowed| !allowed) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied, "publication access revoked"
            )),
            result = operation => Ok(result),
        }
    }
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        let mut readers = READERS.lock().expect("media reader lock");
        if let Some((sender, users)) = readers.get_mut(&self.key) {
            if !sender.subscribe().same_channel(&self.receiver) {
                return;
            }
            *users -= 1;
            if *users == 0 {
                readers.remove(&self.key);
            }
        }
    }
}

fn revoke(key: Key) {
    let mut readers = READERS.lock().expect("media reader lock");
    if let Some((sender, _)) = readers.remove(&key) {
        sender.send_replace(false);
    }
}

fn policy_connection(connected: bool) {
    let mut readers = READERS.lock().expect("media reader lock");
    POLICY_CONNECTED.store(connected, Ordering::Release);
    // A lost listener cannot safely reconstruct which open sessions missed a ban.
    for (_, (sender, _)) in readers.drain() {
        sender.send_replace(false);
    }
}

/// Multi-process deployments listen on the primary PostgreSQL instance. The
/// normal admission checks still read current policy for each new request.
pub async fn watch_policy(db: sea_orm::DatabaseConnection) {
    use sea_orm::sqlx::postgres::PgListener;
    policy_connection(false);
    loop {
        let result = async {
            let mut listener = PgListener::connect_with(db.get_postgres_connection_pool()).await?;
            listener.eager_reconnect(false);
            listener.listen("soundtime_policy").await?;
            policy_connection(true);
            while let Some(message) = listener.try_recv().await? {
                let Some((action, value)) = message.payload().split_once(':') else {
                    continue;
                };
                match action {
                    "track" => {
                        if let Ok(id) = value.parse() {
                            revoke_track(id);
                        }
                    }
                    "raw" => revoke_legacy_hash(value),
                    "peer" => revoke_peer(value),
                    "block" => {
                        revoke_legacy_hash(value);
                        let ids = db
                            .query_all(Statement::from_sql_and_values(
                                DbBackend::Postgres,
                                "SELECT track_id FROM publication_versions WHERE content_hash=$1
                             UNION SELECT id FROM tracks WHERE content_hash=$1",
                                [value.into()],
                            ))
                            .await?;
                        for row in ids {
                            revoke_track(row.try_get("", "track_id")?);
                        }
                    }
                    _ => {}
                }
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        policy_connection(false);
        if let Err(error) = result {
            tracing::warn!(%error,"reconnecting federation policy listener");
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub fn revoke_peer(id: &str) {
    revoke(Key::Peer(id.to_owned()));
}

pub fn revoke_track(id: Uuid) {
    revoke(Key::Track(id));
}

pub fn revoke_legacy_hash(hash: &str) {
    revoke(Key::LegacyHash(hash.to_owned()));
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Allowed,
    Withdrawn,
    Blocked,
    SourceMissing,
}

pub async fn playback_state(
    db: &impl ConnectionTrait,
    id: Uuid,
    hash: Option<&str>,
) -> Result<PlaybackState, DbErr> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT EXISTS(SELECT FROM track_publications WHERE track_id=$1 AND state='withdrawn') AS withdrawn,
                EXISTS(SELECT FROM federation_hidden_publications WHERE track_id=$1)
                    OR EXISTS(SELECT FROM federation_content_blocks WHERE content_hash=$2) AS blocked,
                EXISTS(SELECT FROM federation_missing_sources WHERE track_id=$1) AS missing",
        [id.into(), hash.into()],
    )).await?.ok_or_else(|| DbErr::Custom("no publication decision".into()))?;
    Ok(if row.try_get::<bool>("", "withdrawn")? {
        PlaybackState::Withdrawn
    } else if row.try_get::<bool>("", "blocked")? {
        PlaybackState::Blocked
    } else if row.try_get::<bool>("", "missing")? {
        PlaybackState::SourceMissing
    } else {
        PlaybackState::Allowed
    })
}

/// A legacy hash request cannot distinguish two publications that use the same
/// file. Reject it when it could be a request for a withdrawn or hidden entry.
/// Newer publication-aware transfers can still serve the independent publication.
pub async fn allows_legacy_hash(db: &impl ConnectionTrait, hash: &str) -> Result<bool, DbErr> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT NOT EXISTS(SELECT FROM federation_content_blocks WHERE content_hash=$1)
            AND NOT EXISTS(SELECT FROM publication_versions v
                JOIN track_publications p USING (track_id)
                LEFT JOIN federation_hidden_publications h USING (track_id)
                WHERE v.content_hash=$1 AND (p.state='withdrawn' OR h.track_id IS NOT NULL))
            AND NOT EXISTS(SELECT FROM tracks t
                LEFT JOIN track_publications p ON p.track_id=t.id
                LEFT JOIN federation_hidden_publications h ON h.track_id=t.id
                WHERE t.content_hash=$1 AND (p.state='withdrawn' OR h.track_id IS NOT NULL)) AS allowed",
        [hash.into()],
    )).await?.ok_or_else(|| DbErr::Custom("no legacy transfer decision".into()))?.try_get("", "allowed")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn withdrawal_keeps_independent_readers_of_the_same_file() {
        let first = Uuid::new_v4();
        let other = ReadGuard::track(Uuid::new_v4());
        let guard = ReadGuard::track(first);
        let second_listener = ReadGuard::track(first);
        let raw = ReadGuard::legacy_hash("shared-test-hash");
        revoke_track(first);
        revoke_legacy_hash("shared-test-hash");
        assert!(guard.during(async {}).await.is_err());
        assert!(second_listener.during(async {}).await.is_err());
        assert!(raw.during(async {}).await.is_err());
        assert!(other.during(async {}).await.is_ok());
    }
}
