use super::*;
use sea_orm::QueryResult;
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Request {
    Head,
    Mirrors {
        after: i64,
    },
    Snapshot {
        cut: i64,
        after: Option<Uuid>,
    },
    Changes {
        cut: i64,
        after: i64,
    },
    Withdrawal {
        origin: EndpointId,
        publication: Uuid,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum Response {
    Head(i64),
    Mirrors(Vec<super::lease::JournalEntry>),
    Snapshot { entries: Vec<Entry>, done: bool },
    Changes(Vec<Delta>),
    Withdrawal(Option<Box<SignedPublication>>),
}

// The historical envelope always comes from the frozen journal. Never attach a
// lease to an obsolete or missing local asset just because its old event exists.
fn eligibility() -> &'static str {
    "EXISTS(SELECT FROM track_publications p JOIN tracks t ON t.id=p.track_id
       WHERE p.catalog_id=e.catalog_id AND p.origin_publication_id=e.origin_publication_id AND p.revision=e.revision
         AND p.state='active' AND t.content_hash=e.payload->>'content_hash'
         AND NOT EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=t.id)
         AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=t.id)
         AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=t.content_hash))"
}

fn event(key: &SecretKey, row: &QueryResult) -> Result<Delta> {
    let publication = row.try_get("", "origin_publication_id")?;
    let revision = row.try_get("", "revision")?;
    let cursor = row.try_get("", "cursor")?;
    let kind: String = row.try_get("", "kind")?;
    let payload: Value = row.try_get("", "payload")?;
    let origin = key.public();
    let removed = || Delta {
        cursor,
        change: Change::RemoveOffer {
            origin,
            publication,
            revision,
        },
    };
    let media = if kind == "withdraw" {
        None
    } else {
        ensure!(kind == "upsert", "unsupported catalogue change");
        let Some(hash) = payload["content_hash"].as_str() else {
            return Ok(removed());
        };
        let Some(size) = payload["file_size"].as_i64().filter(|s| *s > 0) else {
            return Ok(removed());
        };
        let title = payload["title"].as_str().context("missing title")?;
        let artist = payload["artist_name"].as_str().context("missing artist")?;
        let format = payload["format"].as_str().context("missing format")?;
        let duration = payload["duration_secs"]
            .as_f64()
            .context("missing duration")?;
        Some(Media {
            title: title.into(),
            artist: artist.into(),
            album: payload["album_title"].as_str().map(str::to_owned),
            format: format.into(),
            duration_ms: (duration * 1000.0).round() as i64,
            size,
            hash: hash.into(),
            genre: payload["genre"].as_str().map(str::to_owned),
            musicbrainz_id: payload["musicbrainz_id"].as_str().map(str::to_owned),
            track_number: serde_json::from_value(payload["track_number"].clone())?,
            disc_number: serde_json::from_value(payload["disc_number"].clone())?,
            year: serde_json::from_value(payload["year"].clone())?,
            bitrate: serde_json::from_value(payload["bitrate"].clone())?,
            sample_rate: serde_json::from_value(payload["sample_rate"].clone())?,
        })
    };
    let offer = media.is_some() && row.try_get::<bool>("", "eligible")?;
    let manifest = Manifest {
        origin,
        publication,
        revision,
        media,
    };
    Ok(Delta {
        cursor,
        change: Change::Put(Box::new(Entry {
            publication: SignedPublication::sign(key, manifest)?,
            offer_until: offer.then(|| chrono::Utc::now().timestamp() + MAX_LEASE_SECS / 2),
        })),
    })
}

pub(crate) async fn head(db: &DatabaseConnection) -> Result<i64> {
    let row = db.query_one(sql("SELECT change_cursor,(SELECT complete FROM federation_event_bootstrap WHERE singleton) AS ready
        FROM federation_catalogs WHERE is_local", [])).await?.context("missing local catalogue")?;
    ensure!(
        row.try_get::<bool>("", "ready")?,
        "historical publication journal is still being prepared"
    );
    Ok(row.try_get("", "change_cursor")?)
}

pub(crate) async fn answer(
    db: &DatabaseConnection,
    key: &SecretKey,
    request: Request,
) -> Result<Response> {
    match request {
        Request::Head => Ok(Response::Head(head(db).await?)),
        Request::Mirrors { after } => {
            Ok(Response::Mirrors(super::lease::current(db, after).await?))
        }
        Request::Snapshot { cut, after } => {
            ensure!((0..=head(db).await?).contains(&cut), "invalid snapshot cut");
            // Apply the keyset before selecting the newest event. Filtering out
            // invalid assets earlier would expose a superseded publication.
            let rows = db.query_all(sql(&format!("SELECT e.*,{} AS eligible FROM
                (SELECT DISTINCT ON (origin_publication_id) * FROM federation_events
                 WHERE catalog_id=(SELECT id FROM federation_catalogs WHERE is_local)
                 AND cursor<=$1 AND ($2::uuid IS NULL OR origin_publication_id>$2)
                 ORDER BY origin_publication_id,cursor DESC) e
                WHERE e.kind='withdraw' OR (e.kind='upsert' AND e.payload->>'content_hash' IS NOT NULL AND (e.payload->>'file_size')::bigint>0)
                ORDER BY origin_publication_id LIMIT $3",eligibility()),
                [cut.into(),after.into(),((PAGE_ENTRIES+1) as i64).into()])).await?;
            let done = rows.len() <= PAGE_ENTRIES;
            let mut entries = Vec::with_capacity(rows.len().min(PAGE_ENTRIES));
            for row in rows.iter().take(PAGE_ENTRIES) {
                match event(key, row)?.change {
                    Change::Put(entry) => entries.push(*entry),
                    _ => bail!("snapshot contains an invalid publication"),
                }
            }
            Ok(Response::Snapshot { entries, done })
        }
        Request::Changes { cut, after } => {
            ensure!(
                after >= 0 && after <= cut && cut <= head(db).await?,
                "invalid changes position"
            );
            let rows = db.query_all(sql(&format!("SELECT e.*,{} AS eligible FROM federation_events e
                WHERE e.catalog_id=(SELECT id FROM federation_catalogs WHERE is_local) AND e.cursor>$1 AND e.cursor<=$2
                ORDER BY e.cursor LIMIT $3",eligibility()), [after.into(),cut.into(),(PAGE_ENTRIES as i64).into()])).await?;
            let changes = rows
                .iter()
                .map(|row| event(key, row))
                .collect::<Result<Vec<_>>>()?;
            Ok(Response::Changes(changes))
        }
        Request::Withdrawal {
            origin,
            publication,
        } => Ok(Response::Withdrawal(
            withdrawal(db, key, origin, publication)
                .await?
                .map(Box::new),
        )),
    }
}

pub(crate) async fn withdrawal(
    db: &DatabaseConnection,
    key: &SecretKey,
    origin: EndpointId,
    publication: Uuid,
) -> Result<Option<SignedPublication>> {
    let row = db.query_one(sql("SELECT p.revision,s.envelope FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
        LEFT JOIN federation_signed_publications s USING(track_id) WHERE c.node_id=$1 AND p.origin_publication_id=$2 AND p.state='withdrawn'",
        [origin.to_string().into(),publication.into()])).await?;
    let Some(row) = row else { return Ok(None) };
    if origin == key.public() {
        return Ok(Some(SignedPublication::sign(
            key,
            Manifest {
                origin,
                publication,
                revision: row.try_get("", "revision")?,
                media: None,
            },
        )?));
    }
    let envelope: Option<Value> = row.try_get("", "envelope")?;
    envelope
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}
