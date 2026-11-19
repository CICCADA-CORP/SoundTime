use super::{clean_one, fingerprint_one};
use anyhow::{ensure, Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension,
};
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use sha2::{Digest, Sha256};
use soundtime_audio::{
    storage::{AnalysisFile, OpenedMedia, StorageError},
    AudioStorage, StorageBackend,
};
use soundtime_db::AppState;
use soundtime_migration::{Migrator, MigratorTrait};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

mod scan;

const PATH: &str = "shared.wav";
const ORIGINAL: &[u8] = b"original audio bytes";
const OTHER: &[u8] = b"replaced audio bytes";

// The normal DELETE handler receives this backend too. Count even existence
// checks and reject whole-file reads: the worker should use bounded range reads.
struct WatchedStorage {
    files: AudioStorage,
    calls: AtomicUsize,
    pause_open: AtomicBool,
    pause_analysis: AtomicBool,
    opened: Notify,
    resume: Notify,
}

impl WatchedStorage {
    fn new(root: &std::path::Path) -> Self {
        Self {
            files: AudioStorage::new(root),
            calls: AtomicUsize::new(0),
            pause_open: AtomicBool::new(false),
            pause_analysis: AtomicBool::new(false),
            opened: Notify::new(),
            resume: Notify::new(),
        }
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl StorageBackend for WatchedStorage {
    async fn open_media(&self, path: &str) -> Result<OpenedMedia, StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.pause_open.swap(false, Ordering::SeqCst) {
            self.opened.notify_one();
            self.resume.notified().await;
        }
        self.files.open_media(path).await
    }

    async fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.files.delete_file(path).await
    }

    async fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.files.file_exists(path).await
    }

    async fn prepare_analysis(&self, path: &str) -> Result<AnalysisFile, StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let file = self.files.prepare_analysis(path).await?;
        if self.pause_analysis.swap(false, Ordering::SeqCst) {
            self.opened.notify_one();
            self.resume.notified().await;
        }
        Ok(file)
    }

    async fn hash_file(&self, path: &str) -> Result<String, StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.files.hash_file(path).await
    }

    fn full_path(&self, _: &str) -> PathBuf {
        panic!("unexpected direct filesystem access")
    }
    async fn read_file(&self, _: &str) -> Result<Vec<u8>, StorageError> {
        panic!("unexpected whole-file read")
    }
    async fn store_file(
        &self,
        _: Uuid,
        _: Option<&str>,
        _: &str,
        _: &[u8],
    ) -> Result<String, StorageError> {
        panic!("unexpected fixture write")
    }
    async fn store_cover(
        &self,
        _: Uuid,
        _: Option<&str>,
        _: &[u8],
    ) -> Result<String, StorageError> {
        panic!("unexpected cover write")
    }
    async fn list_files(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.files.list_files(prefix).await
    }
}

fn state(db: DatabaseConnection, storage: Arc<WatchedStorage>) -> Arc<AppState> {
    Arc::new(AppState {
        db,
        storage,
        jwt_secret: String::new(),
        domain: "withdrawal.test".into(),
        plugins: None,
        p2p: None,
        #[cfg(feature = "redis")]
        redis: None,
    })
}

async fn withdraw(state: Arc<AppState>, owner: Uuid, id: Uuid) -> Result<()> {
    use crate::auth::{
        jwt::{Claims, TokenType},
        middleware::AuthUser,
    };
    let claims = Claims {
        sub: owner,
        username: "test".into(),
        role: "user".into(),
        token_type: TokenType::Access,
        iat: 0,
        exp: 1,
    };
    let status =
        crate::api::tracks::delete_track(State(state), Extension(AuthUser(claims)), Path(id))
            .await
            .map_err(|(status, text)| anyhow::anyhow!("{status} {text}"))?;
    ensure!(status == StatusCode::NO_CONTENT);
    Ok(())
}

async fn status(db: &DatabaseConnection, id: Uuid) -> Result<(String, Option<String>, bool, bool)> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT p.state,t.file_hash,EXISTS(SELECT FROM federation_withdrawn_fingerprints j WHERE j.track_id=t.id) AS pending,
         EXISTS(SELECT FROM federation_file_cleanup j WHERE j.track_id=t.id) AS cleanup
         FROM tracks t JOIN track_publications p ON p.track_id=t.id WHERE t.id=$1", [id.into()]))
        .await?.context("missing publication")?;
    Ok((
        row.try_get("", "state")?,
        row.try_get("", "file_hash")?,
        row.try_get("", "pending")?,
        row.try_get("", "cleanup")?,
    ))
}

async fn make_due(db: &DatabaseConnection, id: Uuid) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE federation_withdrawn_fingerprints SET retry_at=now() WHERE track_id=$1",
        [id.into()],
    ))
    .await?;
    Ok(())
}

async fn exercise(url: &str) -> Result<()> {
    let db = Database::connect(url).await?;
    Migrator::up(&db, None).await?;
    let dir = tempfile::tempdir()?;
    let disk = Arc::new(WatchedStorage::new(dir.path()));
    let owner = Uuid::new_v4();
    let artist = Uuid::new_v4();
    let withdrawn = Uuid::new_v4();
    let active = Uuid::new_v4();
    let unrelated = Uuid::new_v4();
    let external = Uuid::new_v4();
    let sha = format!("{:x}", Sha256::digest(ORIGINAL));
    let blob = soundtime_p2p::BlobHash::new(ORIGINAL).to_string();
    assert_eq!(ORIGINAL.len(), OTHER.len());
    std::fs::write(dir.path().join(PATH), OTHER)?;
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO users(id,username,email,password_hash) VALUES($1,'owner','o@fixture.invalid','no-login')",
        [owner.into()])).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO artists(id,name) VALUES($1,'Test artist')",
        [artist.into()],
    ))
    .await?;
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO tracks(id,title,artist_id,uploaded_by,file_path,file_size,duration_secs,format,content_hash,file_hash)
         VALUES($1,'Old publication',$4,$5,$6,$7,1,'wav',NULL,NULL),
               ($2,'Independent publication',$4,$5,$6,$7,1,'wav',$8,$9),
               ($3,'Unidentified bytes',$4,$5,'unmanaged.wav',$7,1,'wav',NULL,NULL),
               ($10,'Old BLAKE3 only',$4,$5,'external.wav',$7,1,'wav',$8,NULL)",
        [withdrawn.into(),active.into(),unrelated.into(),artist.into(),owner.into(),PATH.into(),
            (ORIGINAL.len() as i64).into(),blob.into(),sha.clone().into(),external.into()])).await?;
    crate::storage_catalog::record_managed(&db, PATH, &sha).await?;

    let api = state(db.clone(), disk.clone());
    withdraw(api.clone(), owner, withdrawn).await?;
    withdraw(api.clone(), owner, unrelated).await?;
    ensure!(
        disk.calls.load(Ordering::SeqCst) == 0,
        "DELETE accessed the original"
    );
    ensure!(status(&db, withdrawn).await? == ("withdrawn".into(), None, true, false));
    ensure!(status(&db, unrelated).await? == ("withdrawn".into(), None, false, false));
    drop(api);
    db.close().await?;

    // Recreate the process state and pool. Pause the first open for hashing;
    // a separate PostgreSQL session must still be able to enter the catalogue.
    let db = Database::connect(url).await?;
    let worker = state(db.clone(), disk.clone());
    let observer = Database::connect(url).await?;
    disk.pause_open.store(true, Ordering::SeqCst);
    let check_writer = async {
        disk.opened.notified().await;
        let tx = observer.begin().await?;
        let row = tx
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT pg_try_advisory_xact_lock(1937011565,1) AS available",
            ))
            .await?
            .context("missing lock probe")?;
        let available: bool = row.try_get("", "available")?;
        tx.rollback().await?;
        disk.resume.notify_one();
        ensure!(available, "hashing blocked an independent catalogue writer");
        Ok::<_, anyhow::Error>(())
    };
    let (processed, writable) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(fingerprint_one(&worker), check_writer)
    })
    .await
    .context("fingerprint worker or independent writer blocked")?;
    ensure!(processed?);
    writable?;
    ensure!(
        status(&db, withdrawn).await? == ("withdrawn".into(), None, true, false),
        "same-size replacement was accepted"
    );
    ensure!(
        !fingerprint_one(&worker).await?,
        "failed verification skipped retry backoff"
    );

    std::fs::write(dir.path().join(PATH), ORIGINAL)?;
    make_due(&db, withdrawn).await?;
    ensure!(fingerprint_one(&worker).await?);
    ensure!(status(&db, withdrawn).await? == ("withdrawn".into(), Some(sha.clone()), false, true));
    let calls_before_cleanup = disk.calls.load(Ordering::SeqCst);
    ensure!(!clean_one(&worker).await?);
    ensure!(
        disk.calls.load(Ordering::SeqCst) == calls_before_cleanup,
        "cleanup accessed shared storage"
    );
    ensure!(status(&db, active).await? == ("active".into(), Some(sha.clone()), false, false));
    ensure!(std::fs::read(dir.path().join(PATH))? == ORIGINAL);
    withdraw(worker.clone(), owner, active).await?;
    ensure!(
        clean_one(&worker).await?,
        "unshared managed original was never cleaned"
    );
    ensure!(!dir.path().join(PATH).exists());

    // A BLAKE3-only scanned original has no managed-file record or historical SHA.
    // Even after confirming the actual SHA, SoundTime has no ownership to unlink it.
    std::fs::write(dir.path().join("external.wav"), OTHER)?;
    withdraw(worker.clone(), owner, external).await?;
    ensure!(fingerprint_one(&worker).await?);
    ensure!(status(&db, external).await? == ("withdrawn".into(), None, true, false));
    std::fs::write(dir.path().join("external.wav"), ORIGINAL)?;
    make_due(&db, external).await?;
    ensure!(fingerprint_one(&worker).await?);
    ensure!(status(&db, external).await? == ("withdrawn".into(), Some(sha), false, false));
    ensure!(std::fs::read(dir.path().join("external.wav"))? == ORIGINAL);
    Ok(())
}

async fn with_fixture(test: impl AsyncFnOnce(&str) -> Result<()>) -> Result<()> {
    // The worker claims the next due job globally. A fresh database prevents it
    // from ever touching another test's withdrawals or configured storage.
    let mut url = url::Url::parse(&std::env::var("TEST_DATABASE_URL")?)?;
    let admin = Database::connect(url.as_str()).await?;
    let fixture = format!("withdrawal_test_{}", Uuid::new_v4().simple());
    admin
        .execute_unprepared(&format!("CREATE DATABASE {fixture}"))
        .await?;
    url.set_path(&format!("/{fixture}"));
    let result = test(url.as_str()).await;
    admin
        .execute_unprepared(&format!("DROP DATABASE {fixture} WITH (FORCE)"))
        .await?;
    result
}

#[tokio::test]
#[ignore = "requires the disposable PostgreSQL federation fixture and permission to create a database"]
async fn historical_withdrawal_defers_original_io_and_preserves_shared_files() -> Result<()> {
    with_fixture(exercise).await
}
