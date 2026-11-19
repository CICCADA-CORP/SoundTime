//! Local database operations for the transition to versioned publications.
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};

/// Bind the local catalogue to its actual iroh identity. A replaced or missing
/// key must not silently transfer the original publications to a different node.
pub async fn bind_local_node(db: &impl ConnectionTrait, node_id: &str) -> Result<(), DbErr> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE federation_catalogs SET node_id = $1
             WHERE is_local AND (node_id IS NULL OR node_id = $1)",
            [node_id.into()],
        ))
        .await?;
    if result.rows_affected() != 1 {
        return Err(DbErr::Custom(
            "local publication catalogue belongs to a different P2P key; restore its original identity".into(),
        ));
    }
    Ok(())
}

/// Project one batch and commit the checkpoint together with the publications.
/// Call again on restart; zero means the initial inventory is complete.
pub async fn backfill_batch(db: &impl ConnectionTrait, batch_size: i32) -> Result<i32, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT federation_backfill_batch($1) AS processed",
        [batch_size.into()],
    ))
    .await?
    .ok_or_else(|| DbErr::Custom("missing federation backfill result".into()))?
    .try_get("", "processed")
}

/// Refresh a bounded page for a renamed artist/album or the initial search index.
pub async fn refresh_search_batch(
    db: &impl ConnectionTrait,
    batch_size: i32,
) -> Result<i32, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT federation_refresh_search_batch($1) AS processed",
        [batch_size.into()],
    ))
    .await?
    .ok_or_else(|| DbErr::Custom("missing catalogue search refresh result".into()))?
    .try_get("", "processed")
}
