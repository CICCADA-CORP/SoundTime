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
    Preparing,
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

// A legacy PostgreSQL row can have an empty display field or a non-finite
// duration. Project those into the wire vocabulary at read time. Never change
// a previously signable value: the same revision must retain its signature.
fn media_from_payload(payload: &Value) -> Option<Media> {
    let hash = payload["content_hash"].as_str()?;
    let size = payload["file_size"].as_i64().filter(|size| *size > 0)?;
    let display = |key: &str| {
        payload[key]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(|name| name.chars().take(255).collect::<String>())
    };
    let format = payload["format"]
        .as_str()
        .filter(|f| !f.is_empty() && f.chars().count() <= 16 && !f.chars().any(char::is_control))
        .unwrap_or("unknown");
    // content_hash and typed integer fields originate from the versioned
    // PostgreSQL projection, including historical backfills.
    let small = |key: &str| payload[key].as_i64().and_then(|n| i16::try_from(n).ok());
    let integer = |key: &str| payload[key].as_i64().and_then(|n| i32::try_from(n).ok());
    Some(Media {
        title: display("title").unwrap_or_else(|| "Untitled".into()),
        artist: display("artist_name").unwrap_or_else(|| "Unknown Artist".into()),
        album: display("album_title"),
        format: format.into(),
        duration_ms: (payload["duration_secs"].as_f64().unwrap_or(0.0).max(0.0) * 1000.0).round()
            as i64,
        size,
        hash: hash.into(),
        genre: payload["genre"]
            .as_str()
            .map(|s| s.chars().take(128).collect()),
        musicbrainz_id: payload["musicbrainz_id"]
            .as_str()
            .filter(|id| id.len() <= 36)
            .map(str::to_owned),
        track_number: small("track_number"),
        disc_number: small("disc_number"),
        year: small("year"),
        bitrate: integer("bitrate"),
        sample_rate: integer("sample_rate"),
    })
}

// Bound the encoded bytes as well as the item count. JSON escaping and
// multibyte metadata can otherwise exceed the frame with fewer than 64 items.
fn bounded_page<T: Serialize>(rows: impl Iterator<Item = Result<T>>) -> Result<(Vec<T>, bool)> {
    // The caller wraps this array in a Response and the receiver in a Page.
    let mut bytes = 1024;
    let mut items = Vec::new();
    for row in rows {
        if items.len() == PAGE_ENTRIES {
            return Ok((items, false));
        }
        let item = row?;
        let size = serde_json::to_vec(&item)?.len() + 1;
        if bytes + size > PAGE_BYTES {
            ensure!(!items.is_empty(), "single catalogue item exceeds page size");
            return Ok((items, false));
        }
        bytes += size;
        items.push(item);
    }
    Ok((items, true))
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
        if kind == "source_missing" {
            return Ok(removed());
        }
        ensure!(
            kind == "upsert" || kind == "source_restored",
            "unsupported catalogue change"
        );
        // A restore followed by a new outage must not recreate the lease.
        if kind == "source_restored" && !row.try_get::<bool>("", "eligible")? {
            return Ok(removed());
        }
        let Some(media) = media_from_payload(&payload) else {
            return Ok(removed());
        };
        Some(media)
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

async fn head(db: &DatabaseConnection) -> Result<Option<i64>> {
    let row = db.query_one(sql("SELECT change_cursor,(SELECT complete FROM federation_event_bootstrap WHERE singleton) AS ready
        FROM federation_catalogs WHERE is_local", [])).await?.context("missing local catalogue")?;
    Ok(row
        .try_get::<bool>("", "ready")?
        .then_some(row.try_get("", "change_cursor")?))
}

pub(crate) async fn answer(
    db: &DatabaseConnection,
    key: &SecretKey,
    request: Request,
) -> Result<Response> {
    match request {
        Request::Head => Ok(head(db).await?.map_or(Response::Preparing, Response::Head)),
        Request::Mirrors { after } => {
            Ok(Response::Mirrors(super::lease::current(db, after).await?))
        }
        Request::Snapshot { cut, after } => {
            let Some(head) = head(db).await? else {
                return Ok(Response::Preparing);
            };
            ensure!((0..=head).contains(&cut), "invalid snapshot cut");
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
            let (entries, done) =
                bounded_page(rows.iter().map(|row| match event(key, row)?.change {
                    Change::Put(entry) => Ok(*entry),
                    _ => bail!("snapshot contains an invalid publication"),
                }))?;
            Ok(Response::Snapshot { entries, done })
        }
        Request::Changes { cut, after } => {
            let Some(head) = head(db).await? else {
                return Ok(Response::Preparing);
            };
            ensure!(
                after >= 0 && after <= cut && cut <= head,
                "invalid changes position"
            );
            // Each arm seeks by the same commit-ordered cursor. Fetch only a
            // page from either index before merging. Availability references
            // the immutable signed subject, not a new publication revision.
            let rows = db.query_all(sql(&format!("WITH ordered AS (
                (SELECT catalog_id,cursor,cursor AS manifest_cursor,NULL::boolean AS present
                   FROM federation_events WHERE catalog_id=(SELECT id FROM federation_catalogs WHERE is_local)
                     AND cursor>$1 AND cursor<=$2 ORDER BY cursor LIMIT $3)
                UNION ALL
                (SELECT catalog_id,cursor,manifest_cursor,present FROM federation_source_events
                  WHERE catalog_id=(SELECT id FROM federation_catalogs WHERE is_local)
                    AND cursor>$1 AND cursor<=$2 ORDER BY cursor LIMIT $3)
            )
            SELECT o.cursor,e.origin_publication_id,e.revision,e.payload,{} AS eligible,
                   CASE WHEN o.present IS TRUE THEN 'source_restored' WHEN o.present IS FALSE THEN 'source_missing' ELSE e.kind END AS kind
              FROM ordered o JOIN federation_events e ON e.catalog_id=o.catalog_id AND e.cursor=o.manifest_cursor
              ORDER BY o.cursor LIMIT $3",eligibility()), [after.into(),cut.into(),(PAGE_ENTRIES as i64).into()])).await?;
            let (changes, _) = bounded_page(rows.iter().map(|row| event(key, row)))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_signable_historical_metadata_keeps_its_signature() -> Result<()> {
        let key = SecretKey::from_bytes(&[9; 32]);
        let hash = "ab".repeat(32);
        let media = Media {
            title: "  Original spelling  ".into(),
            artist: "人".into(),
            album: Some("Album".into()),
            format: "WAV".into(),
            duration_ms: 1234,
            size: 64,
            hash: hash.clone(),
            genre: Some(String::new()),
            musicbrainz_id: Some("à".into()),
            track_number: Some(2),
            disc_number: Some(3),
            year: Some(2025),
            bitrate: Some(48),
            sample_rate: Some(22050),
        };
        let manifest = Manifest {
            origin: key.public(),
            publication: Uuid::new_v4(),
            revision: 1,
            media: Some(media),
        };
        let expected = SignedPublication::sign(&key, manifest.clone())?;
        let payload = serde_json::json!({"title":"  Original spelling  ","artist_name":"人","album_title":"Album","format":"WAV",
            "duration_secs":1.234,"file_size":64,"content_hash":hash,"genre":"","musicbrainz_id":"à","track_number":2,"disc_number":3,
            "year":2025,"bitrate":48,"sample_rate":22050});
        let actual = SignedPublication::sign(
            &key,
            Manifest {
                media: media_from_payload(&payload),
                ..manifest
            },
        )?;
        assert_eq!(serde_json::to_vec(&actual)?, serde_json::to_vec(&expected)?);
        Ok(())
    }

    #[test]
    fn escaped_multibyte_pages_resume_at_first_omitted_item() -> Result<()> {
        let heavy = "🟩\u{1}".repeat(480);
        let rows: Vec<_> = (0..PAGE_ENTRIES).map(|id| (id, heavy.clone())).collect();
        let (first, done) = bounded_page(rows.iter().map(Ok))?;
        assert!(!done && !first.is_empty() && first.len() < PAGE_ENTRIES);
        assert!(serde_json::to_vec(&first)?.len() < PAGE_BYTES);
        let (tail, done) = bounded_page(rows[first.len()..].iter().map(Ok))?;
        assert!(done);
        assert_eq!(tail[0].0, first.len());
        assert_eq!(first.len() + tail.len(), rows.len());
        Ok(())
    }
}
