//! Run production SQL and its real planner in the disposable database.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use soundtime_db::federation::refresh_search_batch;
use std::error::Error;
use uuid::Uuid;

const TRACKS: &str = include_str!("../../src/api/search/tracks.sql");
const ALBUMS: &str = include_str!("../../src/api/search/albums.sql");
const ARTISTS: &str = include_str!("../../src/api/search/artists.sql");
const EXTENDED: &str = include_str!("../../src/api/search/network_results.sql");
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn search_stmt(sql: &str, term: &str, offset: i64, enabled: bool) -> Statement {
    Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        [
            term.into(),
            10_i64.into(),
            offset.into(),
            true.into(),
            enabled.into(),
        ],
    )
}

async fn ids(
    db: &impl ConnectionTrait,
    sql: &str,
    term: &str,
    offset: i64,
    enabled: bool,
) -> Result<Vec<Uuid>> {
    let rows = db
        .query_all(search_stmt(sql, term, offset, enabled))
        .await?;
    Ok(rows
        .iter()
        .map(|row| row.try_get("", "id"))
        .collect::<std::result::Result<_, _>>()?)
}

async fn drain(db: &impl ConnectionTrait, size: i32) -> Result<()> {
    for _ in 0..100 {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT EXISTS (SELECT FROM federation_search_jobs) AS pending",
            ))
            .await?
            .ok_or("missing jobs result")?;
        if !row.try_get::<bool>("", "pending")? {
            return Ok(());
        }
        assert!(refresh_search_batch(db, size).await? <= size);
    }
    Err("search projection did not converge".into())
}

fn used_index(plan: &Value, name: &str) -> bool {
    if plan["Index Name"] == name && plan["Actual Loops"].as_i64().unwrap_or(0) > 0 {
        return true;
    }
    plan["Plans"]
        .as_array()
        .is_some_and(|children| children.iter().any(|child| used_index(child, name)))
}

async fn network(db: &impl ConnectionTrait, item: &Value) -> Result<Vec<Option<Uuid>>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            EXTENDED,
            [json!([item]).into(), true.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .map(|row| row.try_get("", "track_id"))
        .collect::<std::result::Result<_, _>>()?)
}

pub async fn check(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    let scanned: Uuid = "30000000-0000-4000-8000-000000000001".parse()?;
    assert_eq!(ids(&tx, TRACKS, "scanned:*", 0, false).await?, [scanned]);
    drain(&tx, 1).await?;
    assert_eq!(ids(&tx, TRACKS, "scanned:*", 0, false).await?, [scanned]);
    // Reset each bootstrap marker with all current rows already projected.
    // One row per batch detects even a single redundant projection or journal scan.
    tx.execute_unprepared(
        "UPDATE federation_backfill SET last_track_id=NULL, complete=false;
         INSERT INTO federation_search_jobs(kind,subject_id) VALUES('initial','00000000-0000-0000-0000-000000000000');",
    ).await?;
    assert_eq!(soundtime_db::federation::backfill_batch(&tx, 1).await?, 0);
    assert_eq!(refresh_search_batch(&tx, 1).await?, 0);
    tx.execute_unprepared(
        "SELECT federation_seed_events(256);
         UPDATE federation_event_bootstrap SET complete=false,last_track=NULL;",
    )
    .await?;
    let seeded = tx
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT federation_seed_events(1) AS ready",
        ))
        .await?
        .ok_or("missing journal state")?;
    assert!(seeded.try_get::<bool>("", "ready")?);
    tx.execute_unprepared(
        "UPDATE artists SET name='Updated historical artist' WHERE id='20000000-0000-4000-8000-000000000001'",
    ).await?;
    drain(&tx, 1).await?;
    let names = tx
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT count(*) AS count FROM federation_events
         WHERE origin_publication_id='30000000-0000-4000-8000-000000000001'
           AND payload->>'artist_name'='Updated historical artist' AND kind='upsert'",
        ))
        .await?
        .ok_or("no events result")?;
    assert_eq!(names.try_get::<i64>("", "count")?, 1);
    tx.execute_unprepared(include_str!("search_fixture.sql"))
        .await?;
    let row = tx
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id,hash,provider FROM search_fixture WHERE i=1",
        ))
        .await?
        .ok_or("missing seeded track")?;
    let needle: Uuid = row.try_get("", "id")?;
    let hash: String = row.try_get("", "hash")?;
    let provider: String = row.try_get("", "provider")?;
    for sql in [TRACKS, ALBUMS, ARTISTS] {
        let term = if sql == TRACKS {
            "needleuniquefixture:*"
        } else if sql == ALBUMS {
            "cobaltalbum:*"
        } else {
            "amberartist:*"
        };
        assert_eq!(ids(&tx, sql, term, 0, true).await?.len(), 1);
        assert!(ids(&tx, sql, term, 0, false).await?.is_empty());
    }
    for term in ["commonfixture:*", "needleuniquefixture:*"] {
        let plan = tx
            .query_one(search_stmt(
                &format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {TRACKS}"),
                term,
                0,
                true,
            ))
            .await?
            .ok_or("missing search plan")?
            .try_get::<Value>("", "QUERY PLAN")?;
        let root = &plan[0]["Plan"];
        let uses_gin = used_index(root, "federation_search_document_gin");
        println!(
            "{}",
            json!({"phase":"search_explain","catalogues":300,"term":term,"plan":plan})
        );
        if term == "needleuniquefixture:*" && !uses_gin {
            return Err("selective lookup did not execute the document GIN index".into());
        }
    }
    let page1 = ids(&tx, TRACKS, "commonfixture:*", 0, true).await?;
    let page2 = ids(&tx, TRACKS, "commonfixture:*", 10, true).await?;
    assert_eq!(page1.len(), 10);
    assert_eq!(page2.len(), 10);
    assert!(page1.last() < page2.first());
    assert_eq!(ids(&tx, TRACKS, "commonfixture:*", 10, true).await?, page2);
    tx.execute_unprepared("UPDATE tracks SET title='Changedfixture' WHERE id=(SELECT id FROM search_fixture WHERE i=1)").await?;
    assert_eq!(
        ids(&tx, TRACKS, "changedfixture:*", 0, true).await?,
        [needle]
    );
    assert!(ids(&tx, TRACKS, "needleuniquefixture:*", 0, true)
        .await?
        .is_empty());

    // Rename existing parents (no track write). The job must continue across pages.
    tx.execute_unprepared("UPDATE artists SET name='Revisedartist' WHERE name='Amberartist'; UPDATE albums SET title='Revisedalbum' WHERE title='Cobaltalbum'").await?;
    drain(&tx, 128).await?;
    assert_eq!(
        ids(
            &tx,
            TRACKS,
            "changedfixture:* & revisedartist:* & revisedalbum:*",
            0,
            true
        )
        .await?,
        [needle]
    );
    assert!(ids(&tx, TRACKS, "amberartist:*", 0, true).await?.is_empty());

    let mut item = json!({"ordinal":0,"hash":hash,"source_node":provider});
    assert_eq!(network(&tx, &item).await?, [None]);
    item["publication_id"] = json!(needle);
    item["authority_node"] = json!(provider);
    item["revision"] = json!(2);
    assert_eq!(network(&tx, &item).await?, [None]);
    item["revision"] = json!(1);
    assert_eq!(network(&tx, &item).await?, [Some(needle)]);
    tx.execute_unprepared(include_str!("search_policy_checks.sql"))
        .await?;
    assert!(network(&tx, &item).await?.is_empty());
    assert!(ids(&tx, TRACKS, "changedfixture:*", 0, true)
        .await?
        .is_empty());
    tx.execute_unprepared(include_str!("presence_and_legacy.sql"))
        .await?;
    let persist = include_str!("../../../soundtime-p2p/src/persist_presence.sql");
    for (online, sec, expected) in [
        (true, 2, true),
        (false, 1, true),
        (false, 3, false),
        (true, 2, false),
    ] {
        let stamp = format!("2026-11-19T00:00:0{sec}Z");
        let peer = json!([{"id":"ordered-peer","name":null,"version":null,"tracks":3,"online":online,
            "seen":stamp,"checked":stamp}]);
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            persist,
            [peer.into()],
        ))
        .await?;
        let row = tx
            .query_one(Statement::from_string(
                DbBackend::Postgres,
                "SELECT is_online FROM p2p_peers WHERE node_id='ordered-peer'",
            ))
            .await?
            .ok_or("missing durable observation")?;
        assert_eq!(row.try_get::<bool>("", "is_online")?, expected);
    }
    tx.rollback().await?;
    Ok(())
}
