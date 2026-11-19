use super::*;
use sea_orm::Database;

fn release(key: &SecretKey, publication: Uuid, revision: i64) -> Result<Entry> {
    Ok(Entry {
        publication: SignedPublication::sign(
            key,
            Manifest {
                origin: key.public(),
                publication,
                revision,
                media: Some(Media {
                    title: "Inventory fixture".into(),
                    artist: "Inventory fixture".into(),
                    album: None,
                    format: "wav".into(),
                    duration_ms: 250,
                    size: 8000,
                    hash: "b1".repeat(32),
                    ..Default::default()
                }),
            },
        )?,
        offer_until: Some(chrono::Utc::now().timestamp() + 3600),
    })
}

#[test]
fn altered_publication_and_oversized_page_are_rejected() -> Result<()> {
    let key = SecretKey::from_bytes(&[19; 32]);
    let mut entry = release(&key, Uuid::new_v4(), 1)?;
    entry.verify()?;
    let mut forged = entry.clone();
    forged.publication.manifest.media = None;
    forged.offer_until = None;
    assert!(forged.verify().is_err());
    entry.publication.manifest.origin = SecretKey::from_bytes(&[29; 32]).public();
    assert!(entry.verify().is_err());
    assert!(Page::decode(&vec![b' '; PAGE_BYTES + 1]).is_err());
    let entry = release(&key, Uuid::new_v4(), 1)?;
    let mut page = Page {
        snapshot: Uuid::new_v4(),
        cut: 0,
        number: 0,
        last: false,
        entries: vec![],
    };
    assert!(Page::decode(&serde_json::to_vec(&page)?).is_err());
    page.entries = vec![entry; PAGE_ENTRIES + 1];
    assert!(Page::decode(&serde_json::to_vec(&page)?).is_err());
    Ok(())
}

async fn offers(db: &DatabaseConnection, origin: EndpointId, publication: Uuid) -> Result<i64> {
    let row = db.query_one(sql("SELECT count(*) AS n FROM publication_offers o JOIN track_publications p USING(track_id)
        JOIN federation_catalogs c ON c.id=p.catalog_id WHERE c.node_id=$1 AND p.origin_publication_id=$2",
        [origin.to_string().into(),publication.into()])).await?.unwrap();
    Ok(row.try_get("", "n")?)
}

/// Run against the disposable, fully migrated federation test database. Fresh
/// identities isolate concurrent invocations; no application identities are edited.
#[tokio::test]
#[ignore = "requires a disposable TEST_DATABASE_URL with migration 38"]
async fn interrupted_inventory_replay_lease_scoping_and_origin_withdrawal() -> Result<()> {
    let db = Database::connect(std::env::var("TEST_DATABASE_URL")?).await?;
    let origin = SecretKey::generate();
    let a = origin.public();
    let b = SecretKey::generate().public();
    let track = Uuid::new_v4();
    let copy = Uuid::new_v4();
    let one = release(&origin, track, 1)?;
    let two = release(&origin, copy, 1)?;
    for sender in [a, b] {
        crate::peer_policy::follow(&db, &sender.to_string(), true).await?;
        let snapshot = Uuid::new_v4();
        assert_eq!(begin(&db, sender, snapshot, 10).await?, 0);
        let page = serde_json::to_vec(&Page {
            snapshot,
            cut: 10,
            number: 0,
            last: true,
            entries: vec![one.clone(), two.clone()],
        })?;
        apply_page(&db, sender, &page).await?;
        apply_page(&db, sender, &page).await?;
    }
    assert_eq!(offers(&db, a, track).await?, 2);
    let snap = Uuid::new_v4();
    begin(&db, b, snap, 12).await?;
    let page = |number, last, entries| {
        serde_json::to_vec(&Page {
            snapshot: snap,
            cut: 12,
            number,
            last,
            entries,
        })
    };
    assert!(apply_page(&db, b, &page(1, true, vec![])?).await.is_err());
    apply_page(&db, b, &page(0, false, vec![two.clone()])?).await?;
    assert_eq!(offers(&db, a, track).await?, 2);
    assert_eq!(begin(&db, b, snap, 12).await?, 1);
    assert!(apply_page(&db, b, &page(0, false, vec![one.clone()])?)
        .await
        .is_err());
    apply_page(&db, b, &page(1, true, vec![])?).await?;
    assert_eq!(offers(&db, a, track).await?, 1);
    assert_eq!(offers(&db, a, copy).await?, 2);
    let empty = Uuid::new_v4();
    begin(&db, b, empty, 13).await?;
    apply_page(
        &db,
        b,
        &serde_json::to_vec(&Page {
            snapshot: empty,
            cut: 13,
            number: 0,
            last: true,
            entries: vec![],
        })?,
    )
    .await?;
    assert_eq!(offers(&db, a, copy).await?, 1);
    let delta = |cursor| {
        serde_json::to_vec(&Delta {
            cursor,
            change: Change::Put(one.clone()),
        })
    };
    assert!(apply_delta(&db, b, &delta(15)?).await.is_err());
    apply_delta(&db, b, &delta(14)?).await?;
    apply_delta(&db, b, &delta(14)?).await?;
    assert_eq!(offers(&db, a, track).await?, 2);
    let withdrawal = SignedPublication::sign(
        &origin,
        Manifest {
            origin: a,
            publication: track,
            revision: 2,
            media: None,
        },
    )?;
    let bytes = serde_json::to_vec(&withdrawal)?;
    crate::peer_policy::follow(&db, &b.to_string(), false).await?;
    apply_withdrawal(&db, b, &bytes).await?;
    apply_withdrawal(&db, b, &bytes).await?;
    assert_eq!(offers(&db, a, track).await?, 0);
    assert_eq!(offers(&db, a, copy).await?, 1);
    assert!(apply_delta(&db, b, &delta(15)?).await.is_err());
    crate::peer_policy::follow(&db, &b.to_string(), true).await?;
    apply_delta(&db, b, &delta(15)?).await?;
    assert_eq!(offers(&db, a, track).await?, 0);
    db.execute(sql(
        "UPDATE federation_peer_policies SET is_banned=true WHERE node_id=$1",
        [b.to_string().into()],
    ))
    .await?;
    assert!(apply_withdrawal(&db, b, &bytes).await.is_err());
    Ok(())
}
