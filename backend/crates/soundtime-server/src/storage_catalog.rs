//! Coordinate writes to the same music catalog across uploads, scans and server processes.
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr,
    EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use soundtime_db::entities::track;

pub async fn begin_write(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    let txn = db.begin().await?;
    txn.execute_unprepared("SELECT pg_advisory_xact_lock(1937011565, 1)")
        .await?;
    Ok(txn)
}

/// Find the canonical copy while holding the catalog write lock. A file at an
/// indexed path may have been replaced outside SoundTime since the last scan.
pub async fn existing_hash(
    db: &impl ConnectionTrait,
    hash: &str,
    storage: &dyn soundtime_audio::StorageBackend,
) -> Result<Option<track::Model>, String> {
    let rows = track::Entity::find()
        .filter(track::Column::FileHash.eq(hash))
        .filter(track::Column::FilePath.not_like("p2p://%"))
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
            missing.get_or_insert(row);
            continue;
        }
        let actual = storage
            .hash_file(&row.file_path)
            .await
            .map_err(|e| e.to_string())?;
        if actual == hash {
            return Ok(Some(row));
        }
        let mut update: track::ActiveModel = row.into();
        update.file_hash = Set(Some(actual));
        update.content_hash = Set(None);
        update.update(db).await.map_err(|e| e.to_string())?;
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
