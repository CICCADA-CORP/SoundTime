//! Decisions at the byte-serving boundary and cancellation of in-process readers.
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};
use std::{
    collections::HashMap,
    future::Future,
    io,
    sync::{LazyLock, Mutex},
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

/// Register before the database lookup so a withdrawal committed during admission
/// either denies the lookup or wakes the reader. Idle tracks occupy no entries.
pub struct ReadGuard {
    key: Key,
    receiver: watch::Receiver<bool>,
}

impl ReadGuard {
    fn new(key: Key) -> Self {
        let mut readers = READERS.lock().expect("media reader lock");
        let (sender, users) = readers
            .entry(key.clone())
            .or_insert_with(|| (watch::channel(true).0, 0));
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
        if let Some((_, users)) = readers.get_mut(&self.key) {
            *users -= 1;
            if *users == 0 {
                readers.remove(&self.key);
            }
        }
    }
}

fn revoke(key: Key) {
    let readers = READERS.lock().expect("media reader lock");
    if let Some((sender, _)) = readers.get(&key) {
        sender.send_replace(false);
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
