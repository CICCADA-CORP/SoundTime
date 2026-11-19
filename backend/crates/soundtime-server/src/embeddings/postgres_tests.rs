use super::*;
use anyhow::{ensure, Result};
use sea_orm::{ConnectOptions, Database};

#[tokio::test]
#[ignore = "requires migrated PostgreSQL with TEST_DATABASE_URL and pgvector"]
async fn only_originals_are_precomputed_and_remote_seeds_and_listens_are_indexed_on_demand(
) -> Result<()> {
    let mut options = ConnectOptions::new(std::env::var("TEST_DATABASE_URL")?);
    options.min_connections(1).max_connections(1);
    let db = Database::connect(options).await?;
    // Keep the real vector column, uniqueness and HNSW index. All writes use a
    // private session's temporary tables and never alter permanent catalogue data.
    for table in [
        "federation_catalogs",
        "track_publications",
        "tracks",
        "track_embeddings",
        "listen_history",
        "user_taste_vectors",
    ] {
        db.execute_unprepared(&format!(
            "CREATE TEMP TABLE {table} (LIKE public.{table} INCLUDING ALL)"
        ))
        .await?;
    }
    db.execute_unprepared(
        "INSERT INTO federation_catalogs(is_local,node_id) VALUES (true,NULL),(false,'remote');
         INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format)
         SELECT lpad(to_hex(i),32,'0')::uuid,i::text,gen_random_uuid(),
           CASE WHEN i<=64 THEN i::text||'.wav' ELSE 'p2p://remote'||i END,2,60,'wav'
         FROM generate_series(0,69) i;
         INSERT INTO track_publications(track_id,catalog_id,origin_publication_id,revision,state)
         SELECT t.id,c.id,t.id,1,'active' FROM tracks t
         JOIN federation_catalogs c ON c.is_local=(t.file_path NOT LIKE 'p2p://%');",
    )
    .await?;
    // The nil UUID belongs to the first page. The next local original is the
    // only eligible row left despite five remote publications following it.
    let (after, written) = backfill_page(&db, None).await?;
    ensure!(written == BACKFILL_PAGE as u64 && after == Some(Uuid::from_u128(63)));
    let (after, written) = backfill_page(&db, after).await?;
    ensure!(written == 1 && after == Some(Uuid::from_u128(64)));
    ensure!(backfill_page(&db, after).await? == (None, 0));
    ensure!(backfill_page(&db, None).await?.1 == 0);
    let seed = Uuid::from_u128(65);
    let played = Uuid::from_u128(66);
    let cold = Uuid::from_u128(67);
    let user = Uuid::new_v4();
    ensure!(get_track_embedding(&db, seed).await?.is_none());
    let selected = radio_seed_embedding(&db, seed)
        .await?
        .expect("selected remote seed");
    ensure!(!find_similar_tracks(&db, &selected, 3, &[seed])
        .await?
        .is_empty());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO listen_history(id,user_id,track_id,listened_at,duration_listened,completed)
         SELECT gen_random_uuid(),$1::uuid,$2::uuid,now()-interval '1 year',60,true UNION ALL
         SELECT gen_random_uuid(),$1::uuid,$3::uuid,now(),60,true FROM generate_series(1,500)",
        [user.into(), cold.into(), played.into()],
    ))
    .await?;
    update_user_taste_vector(&db, user).await?;
    let taste = get_user_taste_vector(&db, user)
        .await?
        .expect("listener has a taste vector");
    let heard = get_track_embedding(&db, played)
        .await?
        .expect("listened remote track");
    ensure!(taste
        .iter()
        .zip(&heard)
        .all(|(a, b)| (a - b).abs() < 0.00001));
    ensure!(get_track_embedding(&db, cold).await?.is_none());
    // A restart keeps vectors gained through an explicit interaction.
    ensure!(backfill_embeddings(&db).await? == 0);
    ensure!(radio_seed_embedding(&db, seed).await? == Some(selected));
    Ok(())
}
