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
            change: Change::Put(Box::new(one.clone())),
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

#[test]
fn a_mirror_cannot_sign_for_another_provider_or_another_publication() -> Result<()> {
    let a = SecretKey::from_bytes(&[37; 32]);
    let b = SecretKey::from_bytes(&[41; 32]);
    let c = SecretKey::from_bytes(&[43; 32]);
    let original = release(&a, Uuid::new_v4(), 3)?;
    let offer = lease::Offer::issue(&b, original.publication, 1, true)?;
    offer.verify(a.public())?;
    offer.verify(b.public())?;
    assert!(offer.verify(c.public()).is_err());
    let mut other_content = offer.clone();
    other_content.provider.lease.hash = "aa".repeat(32);
    assert!(other_content.verify(b.public()).is_err());
    let mut forged = offer.clone();
    forged.provider.lease.active = false;
    assert!(forged.verify(b.public()).is_err());
    let mut substituted = offer;
    substituted.publication = release(&a, Uuid::new_v4(), 3)?.publication;
    assert!(substituted.verify(b.public()).is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a disposable TEST_DATABASE_URL with migration 40"]
async fn origin_subscriber_accepts_mirror_without_following_it() -> Result<()> {
    let db = Database::connect(std::env::var("TEST_DATABASE_URL")?).await?;
    let a = SecretKey::generate();
    let b = SecretKey::generate();
    let c = SecretKey::generate().public();
    let publication = Uuid::new_v4();
    let main = release(&a, publication, 1)?;
    let independent = release(&a, Uuid::new_v4(), 1)?;
    crate::peer_policy::follow(&db, &a.public().to_string(), true).await?;
    let snap = Uuid::new_v4();
    begin(&db, a.public(), snap, 5).await?;
    apply_page(
        &db,
        a.public(),
        &serde_json::to_vec(&Page {
            snapshot: snap,
            cut: 5,
            number: 0,
            last: true,
            entries: vec![main.clone(), independent.clone()],
        })?,
    )
    .await?;
    let b_id = b.public().to_string();
    let available = |pub_id: Uuid| {
        let issuer = a.public().to_string();
        let provider = b_id.clone();
        let db = &db;
        async move {
            Ok::<_,anyhow::Error>(db.query_one(sql("SELECT 1 FROM federation_track_providers s JOIN track_publications p USING(track_id)
               JOIN federation_catalogs c ON c.id=p.catalog_id WHERE c.node_id=$1 AND p.origin_publication_id=$2 AND s.provider=$3 AND s.allowed AND s.followed AND s.online",
               [issuer.into(),pub_id.into(),provider.into()])).await?.is_some())
        }
    };
    let first = lease::Offer::issue(&b, main.publication.clone(), 1, true)?;
    assert!(lease::accept(&db, c, a.public(), &first, Some(1)).await?);
    assert!(available(publication).await?);
    assert!(!available(independent.publication.manifest.publication).await?);
    assert!(!crate::peer_policy::get(&db, &b_id).await?.followed);
    let gone = lease::Offer::issue(&b, main.publication.clone(), 2, false)?;
    assert!(lease::accept(&db, c, b.public(), &gone, None).await?);
    assert!(!lease::accept(&db, c, b.public(), &first, None).await?);
    assert!(!available(publication).await?);
    let newer = lease::Offer::issue(&b, main.publication, 3, true)?;
    assert!(lease::accept(&db, c, b.public(), &newer, None).await?);
    for _ in 0..2 {
        let key = SecretKey::generate();
        let other = lease::Offer::issue(&key, newer.publication.clone(), 1, true)?;
        assert!(lease::accept(&db, c, a.public(), &other, None).await?);
    }
    let track: Uuid = db
        .query_one(sql(
            "SELECT track_id FROM federation_mirror_leases WHERE provider=$1",
            [b_id.clone().into()],
        ))
        .await?
        .context("missing signed mirror")?
        .try_get("", "track_id")?;
    db.execute(sql("INSERT INTO federation_cached_publications SELECT track_id,revision,content_hash FROM publication_versions WHERE track_id=$1 AND revision=1",
        [track.into()])).await?;
    db.execute(sql("UPDATE federation_mirror_leases SET expires_at=now()-CASE WHEN provider=$2 THEN interval '7 days' ELSE interval '8 days' END WHERE track_id=$1",
        [track.into(),b_id.clone().into()])).await?;
    lease::prune(&db).await?;
    let held = db.query_one(sql("SELECT count(*) AS n,bool_or(provider=$2) AS keeps_b FROM federation_mirror_leases WHERE track_id=$1",
        [track.into(),b_id.clone().into()])).await?.context("missing retention count")?;
    assert_eq!(held.try_get::<i64>("", "n")?, 2);
    assert!(held.try_get::<bool>("", "keeps_b")?);
    assert!(!available(publication).await?);
    db.execute(sql(
        "DELETE FROM federation_cached_publications WHERE track_id=$1",
        [track.into()],
    ))
    .await?;
    lease::prune(&db).await?;
    assert!(db
        .query_one(sql(
            "SELECT 1 FROM federation_mirror_leases WHERE track_id=$1",
            [track.into()]
        ))
        .await?
        .is_none());
    crate::peer_policy::disconnect(&db, &b_id).await?;
    let latest = lease::Offer::issue(&b, newer.publication, 4, true)?;
    lease::accept(&db, c, a.public(), &latest, Some(2)).await?;
    assert!(!available(publication).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL for PostgreSQL"]
async fn preparing_journal_answers_without_publishing_a_partial_head() -> Result<()> {
    use super::source::{answer, Request, Response};
    let mut options = sea_orm::ConnectOptions::new(std::env::var("TEST_DATABASE_URL")?);
    options.max_connections(1).min_connections(1);
    let db = Database::connect(options).await?;
    db.execute_unprepared(
        "CREATE TEMP TABLE federation_catalogs(is_local boolean,change_cursor bigint);
         CREATE TEMP TABLE federation_event_bootstrap(singleton boolean,complete boolean);
         INSERT INTO federation_catalogs VALUES(true,17);
         INSERT INTO federation_event_bootstrap VALUES(true,false)",
    )
    .await?;
    let key = SecretKey::generate();
    for request in [
        Request::Head,
        Request::Snapshot {
            cut: 10,
            after: None,
        },
        Request::Changes { cut: 10, after: 8 },
    ] {
        let frame = serde_json::to_vec(&answer(&db, &key, request).await?)?;
        assert!(matches!(
            serde_json::from_slice(&frame)?,
            Response::Preparing
        ));
    }
    db.execute_unprepared("UPDATE federation_event_bootstrap SET complete=true")
        .await?;
    assert!(matches!(
        answer(&db, &key, Request::Head).await?,
        Response::Head(17)
    ));
    Ok(())
}
