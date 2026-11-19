use super::*;

const NEEDLE: &str = "Rankfixture:*";

async fn check(
    db: &impl ConnectionTrait,
    prefix: &Prefix,
    expected: Option<&[Uuid]>,
) -> Result<()> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            PAGE,
            [
                NEEDLE.into(),
                prefix.snapshot.as_str().into(),
                prefix.ids.clone().into(),
                2_i64.into(),
                0_i64.into(),
                false.into(),
                false.into(),
            ],
        ))
        .await?;
    let usable = rows[0].try_get::<bool>("", "usable")?;
    ensure!(
        usable == expected.is_some(),
        "unexpected reuse for {expected:?}"
    );
    if let Some(expected) = expected {
        let ids = rows
            .iter()
            .filter_map(|r| r.try_get::<Option<Uuid>>("", "id").transpose())
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(ids == expected, "expected {expected:?}, got {ids:?}");
    }
    Ok(())
}

pub(super) async fn exercise(app: &AppState) -> Result<()> {
    let db = &app.db;
    let first = Uuid::from_u128(200);
    let second = Uuid::from_u128(201);
    let promoted = Uuid::from_u128(203);
    db.execute_unprepared(
        "INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format)
         SELECT lpad(to_hex(i),32,'0')::uuid,
             CASE WHEN i<202 THEN 'Rankfixture' ELSE 'Unrelated' END,
             '10000000-0000-0000-0000-000000000001'::uuid,i::text||'.wav',2,1,'wav'
         FROM generate_series(200,234) i",
    )
    .await?;

    let writer = db.begin().await?;
    writer
        .execute_unprepared("SELECT pg_current_xact_id()")
        .await?;
    db.execute_unprepared("SELECT pg_current_xact_id()").await?;
    let initial = rank(app, NEEDLE).await?.context("initial ranking")?;
    writer
        .execute_unprepared("UPDATE tracks SET title='Other' WHERE file_path='202.wav'")
        .await?;
    writer.commit().await?;
    db.execute_unprepared("UPDATE tracks SET title='Different' WHERE file_path='203.wav'")
        .await?;
    check(db, &initial, Some(&[first, second]))
        .await
        .context("old and new transactions changed other terms")?;

    db.execute_unprepared("UPDATE tracks SET title='Other' WHERE file_path='200.wav'")
        .await?;
    check(db, &initial, Some(&[second]))
        .await
        .context("old match removed from term")?;
    db.execute_unprepared(
        "UPDATE tracks SET title='Rankfixture Rankfixture' WHERE file_path='203.wav'",
    )
    .await?;
    check(db, &initial, None)
        .await
        .context("new high-ranking match")?;
    let reordered = rank(app, NEEDLE).await?.context("updated ranking")?;
    check(db, &reordered, Some(&[promoted, second])).await?;
    db.execute_unprepared("UPDATE tracks SET title='Rankfixture' WHERE file_path='203.wav'")
        .await?;
    check(db, &reordered, None)
        .await
        .context("score changed within the term")?;
    let tied = rank(app, NEEDLE).await?.context("equal scores")?;
    check(db, &tied, Some(&[second, promoted])).await?;

    db.execute_unprepared("UPDATE tracks SET title='Other' WHERE file_path='203.wav'")
        .await?;
    let before_bulk = rank(app, NEEDLE)
        .await?
        .context("rank before bulk update")?;
    db.execute_unprepared(
        "UPDATE federation_search_documents SET document=document
         WHERE track_id BETWEEN lpad(to_hex(202),32,'0')::uuid AND lpad(to_hex(233),32,'0')::uuid",
    )
    .await?;
    check(db, &before_bulk, Some(&[second]))
        .await
        .context("32 unrelated documents")?;
    db.execute_unprepared(
        "UPDATE federation_search_documents SET document=document WHERE track_id=lpad(to_hex(234),32,'0')::uuid",
    ).await?;
    check(db, &before_bulk, None)
        .await
        .context("bulk catch-up exceeds the bounded check")?;
    Ok(())
}
