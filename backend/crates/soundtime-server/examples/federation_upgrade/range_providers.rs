use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement, TransactionTrait};
use serde_json::Value;
use std::error::Error;
use uuid::Uuid;

const LOOKUP: &str = include_str!("../../../soundtime-p2p/src/range_transport/providers.sql");
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub async fn check(db: &DatabaseTransaction, track: Uuid, origin: &str, hash: &str) -> Result<()> {
    // The surrounding search fixture has 300 catalogues, one offer for each
    // of 3,000 publications, and fresh table statistics. Isolate policy changes.
    let tx = db.begin().await?;
    let statement = |sql: &str, revision: i64, strict: bool, selected: Option<&str>| {
        Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            [
                track.into(),
                origin.into(),
                track.into(),
                revision.into(),
                hash.into(),
                1024_i64.into(),
                strict.into(),
                12_i64.into(),
                selected.map(str::to_owned).into(),
            ],
        )
    };
    let providers = |revision: i64, strict: bool, selected: Option<&str>| {
        let query = statement(LOOKUP, revision, strict, selected);
        let tx = &tx;
        async move {
            let rows = tx.query_all(query).await?;
            Ok::<_, Box<dyn Error>>(
                rows.iter()
                    .map(|row| row.try_get::<String>("", "provider"))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            )
        }
    };

    let sql = format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {LOOKUP}");
    let plan = tx
        .query_one(statement(&sql, 1, true, None))
        .await?
        .ok_or("missing range plan")?
        .try_get::<Value>("", "QUERY PLAN")?;
    println!(
        "{}",
        serde_json::json!({"phase":"range_provider_explain","tracks":3000,"plan":plan})
    );
    fn broad_scan(node: &Value) -> bool {
        let relation = node["Relation Name"].as_str().unwrap_or("");
        let rows = node["Actual Rows"].as_u64().unwrap_or(0)
            + node["Rows Removed by Filter"].as_u64().unwrap_or(0);
        matches!(relation, "tracks" | "remote_tracks")
            || (matches!(
                relation,
                "track_publications" | "publication_versions" | "publication_offers"
            ) && rows > 64)
            || node["Plans"]
                .as_array()
                .is_some_and(|children| children.iter().any(broad_scan))
    }
    assert!(
        !broad_scan(&plan[0]["Plan"]),
        "range selection scanned unrelated publications or resolved v1"
    );
    assert_eq!(providers(1, true, None).await?, [origin]);
    assert!(providers(2, true, None).await?.is_empty());
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_mirror_leases(track_id,revision,provider,generation,expires_at,active,envelope,manifest)
          VALUES($1,1,'range-fixture-mirror',1,now()+interval '1 hour',true,'{}','{}')",[track.into()])).await?;
    let mirror = "range-fixture-mirror";
    assert_eq!(providers(1, true, None).await?, [origin, mirror]);
    assert_eq!(providers(1, true, Some(mirror)).await?, [mirror]);
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE federation_peer_policies SET is_banned=true WHERE node_id=$1",
        [origin.into()],
    ))
    .await?;
    assert_eq!(providers(1, true, None).await?, [mirror]);
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE federation_peer_policies SET is_followed=false WHERE node_id=$1",
        [origin.into()],
    ))
    .await?;
    assert!(providers(1, true, None).await?.is_empty());
    assert_eq!(providers(1, false, None).await?, [mirror]);
    tx.execute_unprepared(
        "INSERT INTO p2p_peers(node_id,is_online) VALUES('range-fixture-mirror',false)",
    )
    .await?;
    assert!(providers(1, false, None).await?.is_empty());
    tx.execute_unprepared("UPDATE p2p_peers SET is_online=true WHERE node_id='range-fixture-mirror';
        INSERT INTO federation_peer_policies(node_id,source_disabled) VALUES('range-fixture-mirror',true);
        UPDATE federation_catalogs SET node_id='range-fixture-local' WHERE is_local").await?;
    assert!(providers(1, false, None).await?.is_empty());
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_cached_publications(track_id,revision,content_hash) VALUES($1,1,$2)",[track.into(),hash.into()])).await?;
    assert_eq!(providers(1, true, None).await?, ["range-fixture-local"]);
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO federation_content_blocks(content_hash) VALUES($1)",
        [hash.into()],
    ))
    .await?;
    assert!(providers(1, true, None).await?.is_empty());
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM federation_content_blocks WHERE content_hash=$1",
        [hash.into()],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO federation_hidden_publications(track_id) VALUES($1)",
        [track.into()],
    ))
    .await?;
    assert!(providers(1, true, None).await?.is_empty());
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM federation_hidden_publications WHERE track_id=$1",
        [track.into()],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE track_publications SET state='withdrawn' WHERE track_id=$1",
        [track.into()],
    ))
    .await?;
    assert!(providers(1, false, None).await?.is_empty());
    tx.rollback().await?;
    Ok(())
}
