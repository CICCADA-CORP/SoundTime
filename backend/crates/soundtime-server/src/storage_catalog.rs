//! Coordinate writes to the same music catalog across uploads, scans and server processes.
pub(crate) mod releases;

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    QueryFilter, QueryOrder, TransactionTrait,
};
use soundtime_db::entities::track;
use uuid::Uuid;

pub fn reusable_track() -> sea_orm::sea_query::SimpleExpr {
    sea_orm::sea_query::Expr::cust("NOT EXISTS (SELECT 1 FROM track_publications p WHERE p.track_id=tracks.id AND p.state='withdrawn')")
}

pub async fn source_restored(db: &impl ConnectionTrait, id: Uuid) -> Result<(), DbErr> {
    db.execute(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "DELETE FROM federation_missing_sources WHERE track_id=$1",
        [id.into()],
    ))
    .await?;
    Ok(())
}

/// Record explicit ownership only after the storage driver has successfully created the file.
pub async fn record_managed(
    db: &impl ConnectionTrait,
    path: &str,
    hash: &str,
) -> Result<(), DbErr> {
    db.execute(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "INSERT INTO federation_managed_originals(file_path,sha256) VALUES($1,$2)
         ON CONFLICT(file_path) DO UPDATE SET sha256=excluded.sha256",
        [path.into(), hash.into()],
    ))
    .await?;
    Ok(())
}

pub async fn shared_original(
    db: &impl ConnectionTrait,
    hash: &str,
    storage: &dyn soundtime_audio::StorageBackend,
) -> Result<Option<String>, String> {
    let rows = db.query_all(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
        "SELECT m.file_path FROM federation_managed_originals m WHERE m.sha256=$1 AND EXISTS (
            SELECT FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id WHERE t.file_path=m.file_path
            AND t.file_hash=m.sha256 AND p.state IS DISTINCT FROM 'withdrawn'
            AND NOT EXISTS (SELECT FROM federation_missing_sources s WHERE s.track_id=t.id)
        ) ORDER BY m.file_path", [hash.into()])).await.map_err(|e| e.to_string())?;
    for row in rows {
        let path: String = row.try_get("", "file_path").map_err(|e| e.to_string())?;
        if storage
            .file_exists(&path)
            .await
            .map_err(|e| e.to_string())?
            && storage.hash_file(&path).await.map_err(|e| e.to_string())? == hash
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub async fn begin_write(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    let txn = db.begin().await?;
    txn.execute_unprepared("SELECT pg_advisory_xact_lock(1937011565, 1)")
        .await?;
    Ok(txn)
}

pub struct ExistingOriginal {
    pub track: track::Model,
    pub present: bool,
}

pub async fn mark_replaced(db: &impl ConnectionTrait, id: Uuid, path: &str) -> Result<(), DbErr> {
    db.execute(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
        "INSERT INTO federation_missing_sources(track_id)
         SELECT id FROM tracks t WHERE t.id=$1 AND t.file_path=$2
           AND NOT EXISTS(SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn')
         ON CONFLICT DO NOTHING",[id.into(),path.into()])).await?;
    Ok(())
}

/// Find the canonical copy while holding the catalog write lock. A file at an
/// indexed path may have been replaced outside SoundTime since the last scan.
pub async fn existing_hash(
    db: &impl ConnectionTrait,
    hash: &str,
    storage: &dyn soundtime_audio::StorageBackend,
    owner: Option<Uuid>,
) -> Result<Option<ExistingOriginal>, String> {
    let mut query = track::Entity::find()
        .filter(reusable_track())
        .filter(track::Column::FileHash.eq(hash))
        .filter(track::Column::FilePath.not_like("p2p://%"));
    if let Some(owner) = owner {
        query = query.filter(track::Column::UploadedBy.eq(owner));
    }
    let rows = query
        .order_by_asc(track::Column::CreatedAt)
        .order_by_asc(track::Column::Id)
        .all(db)
        .await
        .map_err(|e| e.to_string())?;
    let mut missing = None;
    for row in rows {
        if !storage
            .file_exists(&row.file_path)
            .await
            .map_err(|e| e.to_string())?
        {
            missing.get_or_insert(ExistingOriginal {
                track: row,
                present: false,
            });
            continue;
        }
        let actual = storage
            .hash_file(&row.file_path)
            .await
            .map_err(|e| e.to_string())?;
        if actual == hash {
            source_restored(db, row.id)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(Some(ExistingOriginal {
                track: row,
                present: true,
            }));
        }
        mark_replaced(db, row.id, &row.file_path)
            .await
            .map_err(|e| e.to_string())?;
        missing.get_or_insert(ExistingOriginal {
            track: row,
            present: false,
        });
    }
    Ok(missing)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database, DbBackend, Statement};

    pub async fn assert_postgres_write_lock() {
        let mut options = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
        options.max_connections(1);
        let first = Database::connect(options.clone()).await.unwrap();
        let second = Database::connect(options).await.unwrap();

        let owner = begin_write(&first).await.unwrap();
        let waiter = second.begin().await.unwrap();
        let probe = Statement::from_string(
            DbBackend::Postgres,
            "SELECT pg_try_advisory_xact_lock(1937011565, 1) AS acquired",
        );
        let granted = |row: sea_orm::QueryResult| row.try_get::<bool>("", "acquired").unwrap();
        assert!(!granted(
            waiter.query_one(probe.clone()).await.unwrap().unwrap()
        ));
        owner.commit().await.unwrap();
        assert!(granted(
            waiter.query_one(probe.clone()).await.unwrap().unwrap()
        ));
        waiter.rollback().await.unwrap();
        let next = first.begin().await.unwrap();
        assert!(granted(next.query_one(probe).await.unwrap().unwrap()));
        next.commit().await.unwrap();
    }
}
