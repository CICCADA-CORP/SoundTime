//! Exercise the application's real migration chain against the bench's dedicated database.
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use serde_json::{json, Value};
use soundtime_db::federation::backfill_batch;
use soundtime_migration::{Migrator, MigratorTrait};
use std::error::Error;
use std::fs;

#[path = "federation_upgrade/original_queue.rs"]
mod original_queue;
#[path = "federation_upgrade/projections.rs"]
mod projections;
#[path = "federation_upgrade/range_providers.rs"]
mod range_providers;
#[path = "federation_upgrade/search.rs"]
mod search;

const LEGACY_VERSION: &str = "m20240101_000034_track_file_hash";
const LEGACY_SQL: &str = include_str!("fixtures/federation_legacy.sql");
const REFERENCES_SQL: &str = include_str!("fixtures/federation_references.sql");
const PROJECTIONS_SQL: &str = include_str!("fixtures/federation_projections.sql");
const PRESENT_WAV: &[u8] = b"RIFF\x28\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0data\x04\0\0\0\x7f\x80\x81\x82";
const NEW_IMPORT: &str = "30000000-0000-4000-8000-000000000000";
const ARTIST: &str = "20000000-0000-4000-8000-000000000001";
const OWNER: &str = "10000000-0000-4000-8000-000000000001";
const SCANNED: &str = "30000000-0000-4000-8000-000000000001";
const AMBIGUOUS: &str = "30000000-0000-4000-8000-000000000002";
const MISSING: &str = "30000000-0000-4000-8000-000000000003";

async fn snapshot(db: &DatabaseConnection, sql: &str) -> Result<Value, Box<dyn Error>> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Postgres, sql))
        .await?
        .ok_or("missing references snapshot")?;
    Ok(row.try_get("", "snapshot")?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = std::env::var("TEST_DATABASE_URL")?;
    let db = Database::connect(&database_url).await?;
    let guard = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT current_database() = 'soundtime_federation_history' AND NOT EXISTS \
             (SELECT 1 FROM pg_tables WHERE schemaname = 'public') AS is_fresh_fixture",
        ))
        .await?
        .ok_or("could not identify migration fixture")?;
    if !guard.try_get::<bool>("", "is_fresh_fixture")? {
        return Err(
            "migration fixture requires a fresh, empty soundtime_federation_history database"
                .into(),
        );
    }

    Migrator::up(&db, Some(34)).await?;
    let applied = Migrator::get_applied_migrations(&db).await?;
    if applied.len() != 34 || applied.last().map(|m| m.name()) != Some(LEGACY_VERSION) {
        return Err("historical schema did not stop at migration 34".into());
    }
    let audio = tempfile::tempdir()?;
    fs::create_dir(audio.path().join("scanned"))?;
    fs::write(audio.path().join("scanned/present.wav"), PRESENT_WAV)?;
    std::env::set_var("AUDIO_STORAGE_PATH", audio.path());

    let tx = db.begin().await?;
    tx.execute_unprepared(LEGACY_SQL).await?;
    let scanned_hash = soundtime_p2p::BlobHash::new(PRESENT_WAV).to_string();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE tracks SET content_hash=$1 WHERE id=$2::text::uuid",
        [scanned_hash.clone().into(), SCANNED.into()],
    ))
    .await?;
    tx.commit().await?;
    let before = snapshot(&db, REFERENCES_SQL).await?;
    for (group, expected) in [
        ("tracks", 3),
        ("playlists", 2),
        ("positions", 6),
        ("favorites", 2),
        ("libraries", 1),
        ("memberships", 2),
        ("listens", 2),
    ] {
        if before[group].as_array().map(Vec::len) != Some(expected) {
            return Err(format!("incomplete historical fixture in {group}").into());
        }
    }
    println!(
        "{}",
        json!({"phase":"before", "version":LEGACY_VERSION, "references":before})
    );
    Migrator::up(&db, None).await?;
    if backfill_batch(&db, 1).await? != 1 {
        return Err("expected a single committed historical track in the first batch".into());
    }
    // Drop every pooled connection. The new worker must recover the persisted checkpoint.
    db.close().await?;
    let db = Database::connect(&database_url).await?;
    let writer = Database::connect(&database_url).await?;
    let mut samples = PRESENT_WAV.to_vec();
    *samples
        .last_mut()
        .expect("WAV fixture contains sample data") = 0x83;
    fs::write(audio.path().join("scanned/new.wav"), &samples)?;
    // The new UUID sorts before the last completed batch. The regular track INSERT path
    // must make it visible even when the resumed worker advances its checkpoint.
    let insert = writer.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO tracks (id, title, artist_id, file_path, file_size, duration_secs, format, uploaded_by) \
         VALUES ($1::text::uuid, 'New import', $2::text::uuid, 'scanned/new.wav', 48, 0.0005, 'wav', $3::text::uuid)",
        [NEW_IMPORT.into(), ARTIST.into(), OWNER.into()],
    ));
    tokio::try_join!(insert, backfill_batch(&db, 1))?;
    let mut expected = before;
    expected["tracks"]
        .as_array_mut()
        .ok_or("snapshot is missing track identities")?
        .insert(0, json!([NEW_IMPORT, "New import", ARTIST, OWNER]));

    // Four fixture tracks, one row per batch. A broken worker must not loop indefinitely.
    for _ in 0..4 {
        if backfill_batch(&db, 1).await? == 0 {
            break;
        }
    }
    let mut previous_projection = None;
    for phase in ["upgrade", "replay"] {
        Migrator::up(&db, None).await?;
        if backfill_batch(&db, 1).await? != 0 {
            return Err("historical backfill did not converge".into());
        }
        let applied = Migrator::get_applied_migrations(&db).await?;
        let version = applied.last().ok_or("no migrations applied")?.name();
        let after = snapshot(&db, REFERENCES_SQL).await?;
        let projection = snapshot(&db, PROJECTIONS_SQL).await?;
        println!(
            "{}",
            json!({"phase":phase, "version":version, "references":after, "projections":projection})
        );
        if expected != after {
            return Err(format!("historical references changed during {phase}").into());
        }
        let states = json!([
            [NEW_IMPORT, NEW_IMPORT, true, 1, "active"],
            [SCANNED, SCANNED, true, 1, "active"],
            [AMBIGUOUS, null, false, 1, "unresolved"],
            [MISSING, MISSING, true, 1, "active"],
        ]);
        let versions = json!([
            [NEW_IMPORT, 1, null, 48, "wav"],
            [SCANNED, 1, scanned_hash, 48, "wav"],
            [AMBIGUOUS, 1, null, 2048, "flac"],
            [MISSING, 1, null, 4096, "flac"],
        ]);
        if projection["states"] != states
            || projection["versions"] != versions
            || projection["offers"] != 0
            || projection["local_without_peer_id"] != 1
        {
            return Err(
                "migrated publication identities, versions or offers differ from fixture".into(),
            );
        }
        let events = projection["events"]
            .as_array()
            .ok_or("missing event journal")?;
        if events
            .iter()
            .filter(|event| event[1] == NEW_IMPORT && event[2] == 1 && event[3] == "upsert")
            .count()
            != 1
        {
            return Err("new import must produce exactly one committed publication event".into());
        }
        if previous_projection
            .as_ref()
            .is_some_and(|old| old != &projection)
        {
            return Err("replaying migration/backfill changed publication state or events".into());
        }
        previous_projection = Some(projection);
    }
    original_queue::check(&db).await?;
    projections::check(&db, &writer).await?;
    search::check(&db).await?;
    Ok(())
}
