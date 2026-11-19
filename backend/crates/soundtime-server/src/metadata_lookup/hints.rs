use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};
use soundtime_audio::{recording_tags::RecordingTags, StorageBackend};
use uuid::Uuid;

pub(crate) async fn save(
    db: &impl ConnectionTrait,
    id: Uuid,
    tags: &RecordingTags,
) -> Result<(), DbErr> {
    let recording = tags
        .recording_id
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok());
    if recording.is_none() && tags.isrc.is_none() {
        return Ok(());
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO track_recording_hints(track_id,recording_id,isrc) VALUES($1,$2,$3)
         ON CONFLICT(track_id) DO UPDATE SET recording_id=excluded.recording_id,isrc=excluded.isrc",
        [id.into(), recording.into(), tags.isrc.clone().into()],
    ))
    .await?;
    Ok(())
}

pub(super) async fn load(
    db: &impl ConnectionTrait,
    storage: &dyn StorageBackend,
    id: Uuid,
    path: &str,
) -> Result<(Option<Uuid>, Option<String>), DbErr> {
    if let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT recording_id,isrc FROM track_recording_hints WHERE track_id=$1",
            [id.into()],
        ))
        .await?
    {
        return Ok((row.try_get("", "recording_id")?, row.try_get("", "isrc")?));
    }
    // Existing local uploads can predate tag indexing. Remote catalogues and S3
    // objects have no local tag path; never fetch their media for a metadata search.
    if let Some(path) = storage.local_tag_path(path) {
        if let Ok(Some(tags)) =
            tokio::task::spawn_blocking(move || RecordingTags::read(&path)).await
        {
            save(db, id, &tags).await?;
            return Ok((
                tags.recording_id.and_then(|s| Uuid::parse_str(&s).ok()),
                tags.isrc,
            ));
        }
    }
    Ok((None, None))
}
