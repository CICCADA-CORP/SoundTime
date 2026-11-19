//! Older local imports linked untagged tracks to an album named "Singles" per artist.
//! Reconcile whole, verifiable groups only. An explicitly supplied album of the
//! same name could share the very same row under the (title, artist_id) constraint.
use sea_orm::{ConnectionTrait, DbBackend, DbErr, FromQueryResult, Statement};
use soundtime_audio::StorageBackend;
use soundtime_db::AppState;
use uuid::Uuid;

const CANDIDATE: &str = "a.title='Singles' AND a.release_date IS NULL AND a.musicbrainz_id IS NULL
    AND EXISTS (SELECT FROM legacy_single_albums legacy WHERE legacy.album_id=a.id)
    AND NOT EXISTS (SELECT FROM federation_cache_pins WHERE scope='albums' AND scope_id=a.id)";

#[derive(Debug, PartialEq, FromQueryResult)]
struct Member {
    id: Uuid,
    file_path: String,
    file_hash: Option<String>,
    managed_hash: Option<String>,
    uploaded_by: Option<Uuid>,
    local: bool,
}

impl Member {
    fn verifiable(&self) -> bool {
        let mut parts = self.file_path.split('/');
        let root = parts.next().unwrap_or_default();
        if !self.local
            || root.contains(':')
            || self
                .file_path
                .split('/')
                .any(|p| matches!(p, "" | "." | ".."))
            || self.file_hash.is_none() && self.managed_hash.is_none()
            || matches!((&self.file_hash, &self.managed_hash), (Some(a), Some(b)) if a != b)
        {
            return false;
        }
        match self.uploaded_by {
            // Uploads store the explicit field verbatim (Singles → Singles), but
            // use lowercase singles when the form supplies no album at all.
            Some(owner) => {
                Uuid::parse_str(root) == Ok(owner)
                    && parts.next() == Some("singles")
                    && parts.next().is_some()
                    && parts.next().is_none()
            }
            // An unmanaged scan outside an upload directory has no form overrides.
            None => self.managed_hash.is_none() && Uuid::parse_str(root).is_err(),
        }
    }

    async fn originally_untagged(&self, storage: &dyn StorageBackend) -> bool {
        let source = match storage.prepare_analysis(&self.file_path).await {
            Ok(source) => source,
            Err(error) => {
                tracing::debug!(track=%self.id, %error, "could not verify legacy album metadata");
                return false;
            }
        };
        if Some(&source.sha256) != self.file_hash.as_ref().or(self.managed_hash.as_ref()) {
            return false;
        }
        tokio::task::spawn_blocking(move || {
            soundtime_audio::extract_metadata_from_file(&source.path)
                .is_ok_and(|meta| meta.album.is_none_or(|name| name.trim().is_empty()))
        })
        .await
        .unwrap_or(false)
    }
}

async fn members(db: &impl ConnectionTrait, id: Uuid, lock: bool) -> Result<Vec<Member>, DbErr> {
    let sql = "SELECT t.id,t.file_path,t.file_hash,t.uploaded_by,m.sha256 AS managed_hash,
        ((p.track_id IS NULL OR (c.is_local IS TRUE AND p.state='active')) AND
         NOT EXISTS (SELECT FROM federation_missing_sources f WHERE f.track_id=t.id) AND
         NOT EXISTS (SELECT FROM tracks other WHERE other.file_path=t.file_path AND other.id<>t.id)) AS local
        FROM tracks t LEFT JOIN federation_managed_originals m ON m.file_path=t.file_path
        LEFT JOIN track_publications p ON p.track_id=t.id LEFT JOIN federation_catalogs c ON c.id=p.catalog_id
        WHERE t.album_id=$1 ORDER BY t.id";
    Member::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        if lock {
            format!("{sql} FOR UPDATE OF t")
        } else {
            sql.to_owned()
        },
        [id.into()],
    ))
    .all(db)
    .await
}

async fn repair_group(state: &AppState, id: Uuid) -> Result<u64, DbErr> {
    let original = members(&state.db, id, false).await?;
    if original.is_empty() || original.iter().any(|track| !track.verifiable()) {
        return Ok(0);
    }
    // Filesystem and object-store I/O must not hold a database lock.
    for track in &original {
        if !track.originally_untagged(state.storage.as_ref()).await {
            return Ok(0);
        }
    }

    let tx = crate::storage_catalog::begin_write(&state.db).await?;
    let album = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT a.cover_url FROM albums a WHERE a.id=$1 AND {CANDIDATE} FOR UPDATE"),
            [id.into()],
        ))
        .await?;
    let Some(album) = album else { return Ok(0) };
    // The parent lock also prevents a new FK from being attached during repair.
    if original != members(&tx, id, true).await? {
        return Ok(0);
    }
    let cover: Option<String> = album.try_get("", "cover_url")?;
    let count = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE tracks SET album_id=NULL,cover_url=COALESCE(cover_url,$2) WHERE album_id=$1",
            [id.into(), cover.into()],
        ))
        .await?
        .rows_affected();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM albums a WHERE a.id=$1 AND NOT EXISTS (SELECT FROM tracks WHERE album_id=a.id)",
        [id.into()],
    )).await?;
    tx.commit().await?;
    Ok(count)
}

/// Return the number of detached tracks. A later invocation can retry files
/// unavailable on an earlier pass. Normal track updates drive federation revisions.
pub(crate) async fn reconcile(state: &AppState) -> Result<u64, DbErr> {
    // Tracks and the one-time repair list are known album owners. An extension may
    // have attached another FK with ON DELETE CASCADE; retain its catalogue.
    let extra_references = state.db.query_one(Statement::from_string(DbBackend::Postgres,
        "SELECT EXISTS (SELECT FROM pg_constraint WHERE contype='f' AND confrelid='albums'::regclass
         AND conrelid NOT IN ('tracks'::regclass,'legacy_single_albums'::regclass)) AS external".to_owned())).await?;
    if let Some(row) = extra_references {
        if row.try_get::<bool>("", "external")? {
            return Ok(0);
        }
    }
    let mut last: Option<Uuid> = None;
    let mut total = 0;
    loop {
        let groups = state
            .db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Postgres,
                format!(
                    "SELECT a.id FROM albums a WHERE {CANDIDATE} AND ($1::uuid IS NULL OR a.id>$1)
                     ORDER BY a.id LIMIT 64"
                ),
                [last.into()],
            ))
            .await?;
        if groups.is_empty() {
            break;
        }
        for group in groups {
            let id = group.try_get("", "id")?;
            total += repair_group(state, id).await?;
            last = Some(id);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database};
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    fn wave(album_tag: bool) -> Vec<u8> {
        let mut wav =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0"
                .to_vec();
        if album_tag {
            wav.extend_from_slice(b"LIST\x14\0\0\0INFOIPRD\x08\0\0\0Singles\0");
        }
        wav.extend_from_slice(b"data\x02\0\0\0\x80\x80");
        let size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&size.to_le_bytes());
        wav
    }

    #[tokio::test]
    async fn checks_original_tags_hashes_and_override_directory() {
        let temp = tempfile::tempdir().unwrap();
        let storage = soundtime_audio::AudioStorage::new(temp.path());
        let owner = Uuid::new_v4();
        let bytes = wave(true);
        let mut member = Member {
            id: Uuid::new_v4(),
            file_path: storage
                .store_file(owner, None, "track.wav", &bytes)
                .await
                .unwrap(),
            file_hash: Some(format!("{:x}", Sha256::digest(&bytes))),
            managed_hash: None,
            uploaded_by: Some(owner),
            local: true,
        };
        assert!(member.verifiable());
        assert!(
            !member.originally_untagged(&storage).await,
            "an actual Singles tag"
        );
        let bytes = wave(false);
        std::fs::write(storage.full_path(&member.file_path), &bytes).unwrap();
        assert!(
            !member.originally_untagged(&storage).await,
            "file was replaced"
        );
        member.file_hash = Some(format!("{:x}", Sha256::digest(&bytes)));
        assert!(member.originally_untagged(&storage).await);
        member.file_path = format!("{owner}/Singles/explicit.wav");
        assert!(!member.verifiable());
        member.file_path = "network-nfs/music/track.wav".into();
        member.uploaded_by = None;
        assert!(member.verifiable());
        member.file_hash = None;
        assert!(!member.verifiable());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL for a PostgreSQL connection"]
    async fn reconciles_only_proven_unpinned_groups_with_postgres() {
        let mut options = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.unwrap();
        db.execute_unprepared("CREATE TEMP TABLE albums (id uuid PRIMARY KEY,title text DEFAULT 'Singles',cover_url text DEFAULT '/art/old.jpg',release_date date,musicbrainz_id text);
            CREATE TEMP TABLE legacy_single_albums (album_id uuid PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE);
            CREATE TEMP TABLE tracks (id uuid PRIMARY KEY, album_id uuid REFERENCES albums ON DELETE SET NULL, file_path text, file_hash text, uploaded_by uuid, cover_url text);
            CREATE TEMP TABLE track_publications(track_id uuid PRIMARY KEY,catalog_id uuid,state text);
            CREATE TEMP TABLE federation_catalogs(id uuid PRIMARY KEY,is_local boolean);
            CREATE TEMP TABLE federation_missing_sources(track_id uuid);
            CREATE TEMP TABLE federation_cache_pins(scope text,scope_id uuid);
            CREATE TEMP TABLE federation_managed_originals(file_path text,sha256 text);
            CREATE TEMP TABLE playlist_tracks(track_id uuid REFERENCES tracks,position integer);").await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let storage = Arc::new(soundtime_audio::AudioStorage::new(temp.path()));
        let state = AppState {
            db,
            storage: storage.clone(),
            jwt_secret: String::new(),
            domain: "localhost".into(),
            p2p: None,
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
        };
        let owner = Uuid::new_v4();
        let mut detached = Vec::new();
        for kind in [
            "fallback",
            "mixed",
            "enriched",
            "pin",
            "override",
            "unreadable",
            "new",
        ] {
            let id = Uuid::new_v4();
            state
                .db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "INSERT INTO albums(id,musicbrainz_id) VALUES($1,$2)",
                    [
                        id.into(),
                        (kind == "enriched").then_some("release-id").into(),
                    ],
                ))
                .await
                .unwrap();
            if kind != "new" {
                state
                    .db
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO legacy_single_albums VALUES($1)",
                        [id.into()],
                    ))
                    .await
                    .unwrap();
            }
            if kind == "pin" {
                state
                    .db
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO federation_cache_pins VALUES('albums',$1)",
                        [id.into()],
                    ))
                    .await
                    .unwrap();
            }
            for index in 0..2 {
                let track = Uuid::new_v4();
                let bytes = wave(kind == "mixed" && index == 1);
                let path = storage
                    .store_file(
                        owner,
                        (kind == "override").then_some("Singles"),
                        &format!("{track}.wav"),
                        &bytes,
                    )
                    .await
                    .unwrap();
                if kind == "unreadable" {
                    storage.delete_file(&path).await.unwrap();
                }
                state
                    .db
                    .execute(Statement::from_sql_and_values(
                        DbBackend::Postgres,
                        "INSERT INTO tracks VALUES($1,$2,$3,$4,$5,$6)",
                        [
                            track.into(),
                            id.into(),
                            path.into(),
                            format!("{:x}", Sha256::digest(bytes)).into(),
                            owner.into(),
                            (index == 1).then_some("/art/own.jpg").into(),
                        ],
                    ))
                    .await
                    .unwrap();
                if kind == "fallback" {
                    state
                        .db
                        .execute(Statement::from_sql_and_values(
                            DbBackend::Postgres,
                            "INSERT INTO playlist_tracks VALUES($1,$2)",
                            [track.into(), index.into()],
                        ))
                        .await
                        .unwrap();
                    detached.push(track);
                }
            }
        }
        assert_eq!(reconcile(&state).await.unwrap(), 2);
        assert_eq!(reconcile(&state).await.unwrap(), 0);
        let linked = state.db.query_all(Statement::from_string(DbBackend::Postgres,
            "SELECT t.id,t.album_id,t.cover_url FROM tracks t JOIN playlist_tracks pt ON pt.track_id=t.id ORDER BY pt.position".to_owned())).await.unwrap();
        assert_eq!(linked.len(), 2);
        for (index, row) in linked.into_iter().enumerate() {
            assert_eq!(row.try_get::<Uuid>("", "id").unwrap(), detached[index]);
            assert_eq!(row.try_get::<Option<Uuid>>("", "album_id").unwrap(), None);
            assert_eq!(
                row.try_get::<String>("", "cover_url").unwrap(),
                if index == 0 {
                    "/art/old.jpg"
                } else {
                    "/art/own.jpg"
                }
            );
        }
        let groups = state
            .db
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT count(*) AS remaining FROM albums".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(groups.try_get::<i64>("", "remaining").unwrap(), 6);
    }
}
