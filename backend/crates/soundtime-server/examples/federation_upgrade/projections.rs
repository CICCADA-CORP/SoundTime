//! Publication regressions on the disposable v34 -> current migration fixture.
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, RuntimeErr, SqlxError, Statement,
    TransactionTrait,
};
use soundtime_db::federation::bind_local_node;
use std::error::Error;
use uuid::Uuid;

async fn append(
    db: &impl ConnectionTrait,
    catalog: Uuid,
    publication: Uuid,
    revision: i64,
) -> Result<i64, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT federation_append_event($1, $2, $3, 'upsert', '{}'::jsonb) AS cursor",
        [catalog.into(), publication.into(), revision.into()],
    ))
    .await?
    .ok_or_else(|| DbErr::Custom("missing event cursor".into()))?
    .try_get("", "cursor")
}

pub async fn check(
    db: &DatabaseConnection,
    writer: &DatabaseConnection,
) -> Result<(), Box<dyn Error>> {
    let tx = db.begin().await?;
    bind_local_node(&tx, &"1".repeat(64)).await?;
    bind_local_node(&tx, &"1".repeat(64)).await?;
    if bind_local_node(&tx, &"2".repeat(64)).await.is_ok() {
        return Err("different key rebound existing origin".into());
    }
    tx.execute_unprepared(include_str!("write_checks.sql"))
        .await?;
    tx.rollback().await?;

    let catalog = Uuid::new_v4();
    let publication = Uuid::new_v4();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO federation_catalogs(id, node_id) VALUES ($1, $2)",
        [catalog.into(), catalog.to_string().into()],
    ))
    .await?;
    let abandoned = db.begin().await?;
    assert_eq!(append(&abandoned, catalog, publication, 1).await?, 1);
    abandoned.rollback().await?;

    let first = db.begin().await?;
    assert_eq!(append(&first, catalog, publication, 1).await?, 1);
    let competing = writer.begin().await?;
    competing
        .execute_unprepared("SET LOCAL lock_timeout = '300ms'")
        .await?;
    let blocked = append(&competing, catalog, publication, 2).await;
    // Require the specific PostgreSQL lock timeout, rather than accepting an
    // unrelated error (e.g. a bad query) as evidence of commit serialization.
    match blocked {
        Err(DbErr::Query(RuntimeErr::SqlxError(SqlxError::Database(error))))
            if error.code().as_deref() == Some("55P03") => {}
        result => {
            return Err(format!("expected catalogue cursor lock timeout, got {result:?}").into())
        }
    }
    competing.rollback().await?;
    first.commit().await?;
    assert_eq!(append(writer, catalog, publication, 1).await?, 1);
    assert_eq!(append(writer, catalog, publication, 2).await?, 2);
    Ok(())
}
