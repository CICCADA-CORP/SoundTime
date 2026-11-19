use super::*;
use crate::api::search::{search, SearchParams};
use anyhow::{ensure, Context, Result};
use axum::extract::{Query, State};
use sea_orm::{Database, TransactionTrait};
use soundtime_migration::{Migrator, MigratorTrait};

const TERM: &str = "Gemmix:*";
const ADD_FIRST: &str = "INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format)
    VALUES ('00000000-0000-0000-0000-000000000000','Gemmix','10000000-0000-0000-0000-000000000001','first.wav',2,1,'wav')";

async fn api_ids(app: &Arc<AppState>, page: u64) -> Result<Vec<Uuid>> {
    let hits = search(
        State(app.clone()),
        Query(SearchParams {
            q: "Gemmix".into(),
            page: Some(page),
            per_page: Some(10),
            include_p2p: Some(false),
        }),
    )
    .await
    .map_err(|(status, message)| anyhow::anyhow!("{status} {message}"))?;
    Ok(hits.tracks.iter().map(|t| t.id).collect())
}

fn ids(start: u128, count: usize) -> Vec<Uuid> {
    (start..).take(count).map(Uuid::from_u128).collect()
}

async fn exercise(url: &str) -> Result<()> {
    let db = Database::connect(url).await?;
    Migrator::up(&db, None).await?;
    db.execute_unprepared(
        "INSERT INTO artists(id,name) VALUES ('10000000-0000-0000-0000-000000000001','Artist');
        INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format)
        SELECT lpad(to_hex(i),32,'0')::uuid,'Gemmix','10000000-0000-0000-0000-000000000001'::uuid,
               i::text||'.wav',2,1,'wav' FROM generate_series(1,80) i;
        DELETE FROM federation_search_jobs WHERE kind='initial'",
    )
    .await?;
    let dir = tempfile::tempdir()?;
    let app = Arc::new(AppState {
        db: db.clone(),
        storage: Arc::new(soundtime_audio::AudioStorage::new(dir.path())),
        domain: "ranking.test".into(),
        jwt_secret: String::new(),
        plugins: None,
        p2p: None,
        #[cfg(feature = "redis")]
        redis: None,
    });

    // This writer already has an XID when the ranking is captured. Its commit is
    // invisible to a test that compares only the highest XID seen so far.
    let writer = db.begin().await?;
    writer
        .execute_unprepared("SELECT pg_current_xact_id()")
        .await?;
    db.execute_unprepared("SELECT pg_current_xact_id()").await?;
    let (one, two) = tokio::try_join!(api_ids(&app, 1), api_ids(&app, 1))?;
    ensure!(one == ids(1, 10) && two == one);
    let key = (Arc::as_ptr(&app) as usize, TERM.to_owned());
    let slot = CACHE
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .context("missing shared rank")?;
    let prefix = slot
        .prefix
        .get()
        .and_then(Option::as_ref)
        .context("missing ranked prefix")?;
    ensure!(!prefix.complete && prefix.ids.as_array().unwrap().len() == PREFIX);
    writer.execute_unprepared(ADD_FIRST).await?;
    writer.commit().await?;
    ensure!(
        api_ids(&app, 1).await? == ids(0, 10),
        "new lower UUID was omitted"
    );
    ensure!(api_ids(&app, 1).await? == ids(0, 10));

    // Rights and removals from the index are independent of a document insert.
    db.execute_unprepared(
        "INSERT INTO federation_hidden_publications(track_id)
        SELECT lpad(to_hex(i),32,'0')::uuid FROM generate_series(0,3) i",
    )
    .await?;
    ensure!(api_ids(&app, 2).await? == ids(14, 10));
    db.execute_unprepared(
        "DELETE FROM federation_search_documents
        WHERE track_id='00000000-0000-0000-0000-000000000004'",
    )
    .await?;
    ensure!(api_ids(&app, 1).await? == ids(5, 10));
    db.execute_unprepared(
        "INSERT INTO federation_hidden_publications(track_id)
        SELECT lpad(to_hex(i),32,'0')::uuid FROM generate_series(5,63) i",
    )
    .await?;
    ensure!(
        api_ids(&app, 1).await? == ids(64, 10),
        "did not continue beyond filtered prefix"
    );
    db.execute_unprepared("DELETE FROM federation_hidden_publications")
        .await?;
    ensure!(
        api_ids(&app, 8).await? == ids(71, 10),
        "deep page differs from the catalogue order"
    );

    db.execute_unprepared(
        "UPDATE tracks SET title='Mistraline' WHERE id='00000000-0000-0000-0000-000000000000'",
    )
    .await?;
    ensure!(api_ids(&app, 1).await?.first() == Some(&Uuid::from_u128(1)));
    db.execute_unprepared(
        "INSERT INTO federation_search_jobs(kind,subject_id)
        VALUES('initial','00000000-0000-0000-0000-000000000000')",
    )
    .await?;
    ensure!(
        api_ids(&app, 1).await? == ids(1, 10),
        "initial projection must use live tracks"
    );
    db.execute_unprepared("DELETE FROM federation_search_jobs WHERE kind='initial'")
        .await?;
    // The writer has withdrawn this publication, but the committed index and
    // policy still expose it while the shared prefix is being computed.
    let removing = db.begin().await?;
    removing
        .execute_unprepared(
            "SELECT federation_withdraw_local('00000000-0000-0000-0000-000000000000')",
        )
        .await?;
    let (visible, _) = page(&app, "Mistraline:*", 10, 0, [false, false])
        .await?
        .context("missing complete prefix")?;
    ensure!(visible.len() == 1 && visible[0].try_get::<Uuid>("", "id")? == Uuid::nil());
    removing.commit().await?;
    let (after, _) = page(&app, "Mistraline:*", 10, 0, [false, false])
        .await?
        .context("withdrawal only changed policy")?;
    ensure!(
        after.is_empty(),
        "a shared UUID ranking allowed a withdrawn publication"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with permission to create a private database"]
async fn cached_rank_preserves_current_order_visibility_and_pages() -> Result<()> {
    let mut url = url::Url::parse(&std::env::var("TEST_DATABASE_URL")?)?;
    let admin = Database::connect(url.as_str()).await?;
    let fixture = format!("rank_test_{}", Uuid::new_v4().simple());
    admin
        .execute_unprepared(&format!("CREATE DATABASE {fixture}"))
        .await?;
    url.set_path(&format!("/{fixture}"));
    let result = exercise(url.as_str()).await;
    admin
        .execute_unprepared(&format!("DROP DATABASE {fixture} WITH (FORCE)"))
        .await?;
    result
}
