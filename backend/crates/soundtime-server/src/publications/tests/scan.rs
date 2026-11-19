use super::*;
use axum::{body::to_bytes, http::HeaderMap, response::IntoResponse};
use sea_orm::{EntityTrait, PaginatorTrait};
use soundtime_db::entities::track;

const WAV: &[u8] = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0data\x04\0\0\0\x7f\x80\x81\x82";

async fn playlist_status(
    app: &Arc<AppState>,
    playlist: Uuid,
    id: Uuid,
    expected: &str,
) -> Result<()> {
    let playlist = crate::api::playlists::get_playlist(State(app.clone()), Path(playlist), None)
        .await
        .map_err(|(status, why)| anyhow::anyhow!("{status} {why}"))?;
    let [entry] = playlist.tracks.as_slice() else {
        anyhow::bail!("playlist lost or duplicated the entry");
    };
    ensure!(
        entry.id == id && entry.position == Some(0) && entry.availability == expected,
        "unexpected playlist entry {entry:?}"
    );
    Ok(())
}

async fn withdrawn(app: &Arc<AppState>, playlist: Uuid, id: Uuid) -> Result<()> {
    playlist_status(app, playlist, id, "withdrawn").await?;
    let stream = crate::api::audio::stream_track(State(app.clone()), Path(id), HeaderMap::new())
        .await
        .into_response();
    ensure!(
        stream.status() == StatusCode::GONE,
        "withdrawn stream returned {}",
        stream.status()
    );
    Ok(())
}

async fn scan_without_import(app: &AppState, scanned: u64) -> Result<()> {
    let report = crate::storage_worker::run_sync(app, None, &[])
        .await
        .map_err(anyhow::Error::msg)?;
    ensure!(
        report.errors.is_empty()
            && report.scanned == scanned
            && report.skipped == scanned
            && report.imported == 0
            && report.moved == 0
            && report.removed == 0,
        "unexpected scan {report:?}"
    );
    Ok(())
}

enum HistoricalFingerprint {
    Blob,
    ManagedSha,
}

async fn exercise_scan(url: &str, fingerprint: HistoricalFingerprint) -> Result<()> {
    let db = Database::connect(url).await?;
    Migrator::up(&db, None).await?;
    let dir = tempfile::tempdir()?;
    let disk = Arc::new(WatchedStorage::new(dir.path()));
    let app = state(db.clone(), disk.clone());
    let (owner, other_owner, artist, id, independent, playlist) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO users(id,username,email,password_hash) VALUES
         ($1,'first','first@fixture.invalid','no-login'),($2,'other','other@fixture.invalid','no-login')",
        [owner.into(), other_owner.into()])).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO artists(id,name) VALUES($1,'Artist')",
        [artist.into()],
    ))
    .await?;
    let blob = soundtime_p2p::BlobHash::new(WAV).to_string();
    let sha = format!("{:x}", Sha256::digest(WAV));
    let publication = |id: Uuid, owner: Uuid, path: &str, blob: Option<&str>, sha: Option<&str>| {
        Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO tracks(id,title,artist_id,uploaded_by,file_path,file_size,duration_secs,format,content_hash,file_hash)
             VALUES ($1,$2,$3,$4,$2,$5,1,'wav',$6,$7)",
            [id.into(),path.into(),artist.into(),owner.into(),(WAV.len() as i64).into(),blob.into(),sha.into()])
    };
    std::fs::write(dir.path().join("original.wav"), WAV)?;
    let (track_blob, managed_sha) = match fingerprint {
        HistoricalFingerprint::Blob => (Some(blob.as_str()), None),
        HistoricalFingerprint::ManagedSha => {
            crate::storage_catalog::record_managed(&db, "original.wav", &sha).await?;
            (None, Some(sha.as_str()))
        }
    };
    db.execute(publication(id, owner, "original.wav", track_blob, None))
        .await?;
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "WITH saved AS (INSERT INTO playlists(id,user_id,name,is_public) VALUES($1,$2,'Saved',true) RETURNING id)
         INSERT INTO playlist_tracks(playlist_id,track_id,position) SELECT id,$3,0 FROM saved",
        [playlist.into(),owner.into(),id.into()])).await?;
    playlist_status(&app, playlist, id, "playable").await?;

    // The scanner sees a historical original moved before its first SHA backfill.
    // Its snapshot is stale by the time it may acquire the catalogue lock.
    std::fs::rename(
        dir.path().join("original.wav"),
        dir.path().join("moved.wav"),
    )?;
    disk.pause_analysis.store(true, Ordering::SeqCst);
    let api_db = Database::connect(url).await?;
    let api = state(api_db.clone(), disk.clone());
    let deleting = async {
        disk.opened.notified().await;
        withdraw(api.clone(), owner, id).await?;
        withdrawn(&api, playlist, id).await?;
        let job = api_db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT expected_sha256 FROM federation_withdrawn_fingerprints WHERE track_id=$1",
                [id.into()],
            ))
            .await?
            .context("withdrawal did not preserve the fingerprint job")?;
        ensure!(
            job.try_get::<Option<String>>("", "expected_sha256")?
                .as_deref()
                == managed_sha
        );
        disk.resume.notify_one();
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        tokio::try_join!(scan_without_import(&app, 1), deleting)
    })
    .await
    .context("scan and DELETE did not reach the storage barrier or make independent progress")??;
    withdrawn(&app, playlist, id).await?;
    ensure!(track::Entity::find().count(&db).await? == 1);
    drop(api);
    drop(app);
    drop(disk);
    api_db.close().await?;
    db.close().await?;

    // Start over with fresh storage and PostgreSQL connections. There is no
    // active copy to suppress a duplicate import: the persisted withdrawal must.
    std::fs::rename(
        dir.path().join("moved.wav"),
        dir.path().join("rediscovered.wav"),
    )?;
    let db = Database::connect(url).await?;
    let app = state(db.clone(), Arc::new(WatchedStorage::new(dir.path())));
    scan_without_import(&app, 1).await?;
    withdrawn(&app, playlist, id).await?;
    ensure!(track::Entity::find().count(&db).await? == 1);

    // A separately authored publication points to the same bytes on its own path.
    // Withdrawing the first UUID must neither transfer its playlist slot nor deny the second stream.
    std::fs::write(dir.path().join("independent.wav"), WAV)?;
    db.execute(publication(
        independent,
        other_owner,
        "independent.wav",
        Some(&blob),
        Some(&sha),
    ))
    .await?;
    scan_without_import(&app, 2).await?;
    ensure!(track::Entity::find().count(&db).await? == 2);
    withdrawn(&app, playlist, id).await?;
    let response = crate::api::audio::stream_track(State(app), Path(independent), HeaderMap::new())
        .await
        .into_response();
    ensure!(response.status() == StatusCode::OK);
    ensure!(to_bytes(response.into_body(), WAV.len()).await?.as_ref() == WAV);
    Ok(())
}

#[tokio::test]
#[ignore = "requires the disposable PostgreSQL federation fixture and permission to create a database"]
async fn concurrent_scan_and_rescan_cannot_revive_a_withdrawn_publication() -> Result<()> {
    with_fixture(async |url| exercise_scan(url, HistoricalFingerprint::Blob).await).await
}

#[tokio::test]
#[ignore = "requires the disposable PostgreSQL federation fixture and permission to create a database"]
async fn concurrent_scan_honors_the_withdrawn_managed_sha_before_confirmation() -> Result<()> {
    with_fixture(async |url| exercise_scan(url, HistoricalFingerprint::ManagedSha).await).await
}
