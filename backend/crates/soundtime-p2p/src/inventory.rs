//! Publication manifests and transactional inventory reception. The caller owns
//! the authenticated transport and starts each snapshot after negotiating its cut.
use anyhow::{bail, ensure, Context, Result};
use iroh::{EndpointId, SecretKey, Signature};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PAGE_BYTES: usize = 256 * 1024;
pub const PAGE_ENTRIES: usize = 64;
const MAX_PAGES: i32 = 16384;
const MAX_LEASE_SECS: i64 = 24 * 60 * 60;
const SIGNING_DOMAIN: &[u8] = b"soundtime/publication/1\0";
pub mod lease;
pub(crate) mod source;
pub const ALPN: &[u8] = b"soundtime/catalog/2";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub format: String,
    pub duration_ms: i64,
    pub size: i64,
    /// Lowercase hexadecimal iroh BLAKE3 root, independent of this publication.
    pub hash: String,
    pub genre: Option<String>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub year: Option<i16>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub musicbrainz_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub origin: EndpointId,
    pub publication: Uuid,
    pub revision: i64,
    /// None declares a permanent, signed withdrawal.
    pub media: Option<Media>,
}

impl Manifest {
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        ensure!(
            self.revision > 0 && !self.publication.is_nil(),
            "invalid publication revision or ID"
        );
        if let Some(m) = &self.media {
            for name in [&m.title, &m.artist].into_iter().chain(m.album.as_ref()) {
                ensure!(
                    !name.is_empty() && name.chars().count() <= 255 && !name.contains('\0'),
                    "invalid track metadata"
                );
            }
            ensure!(
                !m.format.is_empty()
                    && m.format.chars().count() <= 16
                    && !m.format.chars().any(char::is_control),
                "invalid track format"
            );
            ensure!(
                m.duration_ms >= 0 && m.size > 0,
                "invalid track size or duration"
            );
            ensure!(
                m.genre
                    .as_deref()
                    .is_none_or(|s| s.chars().count() <= 128 && !s.contains('\0'))
                    && m.musicbrainz_id
                        .as_deref()
                        .is_none_or(|s| s.len() <= 36 && !s.contains('\0')),
                "invalid extended metadata"
            );
            ensure!(
                m.hash.len() == 64 && m.hash.bytes().all(|b| matches!(b,b'0'..=b'9'|b'a'..=b'f')),
                "invalid BLAKE3 root"
            );
        }
        let mut bytes = SIGNING_DOMAIN.to_vec();
        // Fixed field order, typed integer fields and no unordered objects.
        serde_json::to_writer(&mut bytes, self)?;
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedPublication {
    pub manifest: Manifest,
    pub signature: Signature,
}

impl SignedPublication {
    pub fn sign(key: &SecretKey, manifest: Manifest) -> Result<Self> {
        ensure!(
            key.public() == manifest.origin,
            "incorrect publication signing identity"
        );
        let signature = key.sign(&manifest.signing_bytes()?);
        Ok(Self {
            manifest,
            signature,
        })
    }

    pub fn verify(&self) -> Result<()> {
        self.manifest
            .origin
            .verify(&self.manifest.signing_bytes()?, &self.signature)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub publication: SignedPublication,
    /// The authenticated *provider* takes responsibility for this lease. A
    /// signed manifest alone never implies that the sender stores the bytes.
    pub offer_until: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub snapshot: Uuid,
    pub cut: i64,
    pub number: i32,
    pub last: bool,
    pub entries: Vec<Entry>,
}

impl Page {
    fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= PAGE_BYTES, "inventory page too large");
        let page: Self = serde_json::from_slice(bytes)?;
        ensure!(
            !page.snapshot.is_nil() && page.cut >= 0 && (0..MAX_PAGES).contains(&page.number),
            "invalid inventory position"
        );
        ensure!(
            page.entries.len() <= PAGE_ENTRIES && (page.last || !page.entries.is_empty()),
            "invalid inventory page length"
        );
        ensure!(
            page.last || page.number < MAX_PAGES - 1,
            "inventory exceeds page budget"
        );
        for entry in &page.entries {
            entry.verify()?;
        }
        Ok(page)
    }
}

fn sql(text: &str, params: impl IntoIterator<Item = sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Postgres, text, params)
}

async fn lock_policy(
    tx: &DatabaseTransaction,
    provider: &str,
    require_follow: bool,
) -> Result<i64> {
    let row = tx.query_one(sql("SELECT p.is_followed,p.applied_cursor FROM federation_peer_policies p
        WHERE node_id=$1 AND NOT is_banned AND NOT EXISTS(SELECT FROM blocked_domains b WHERE b.domain=p.node_id)
        FOR UPDATE OF p", [provider.into()])).await?.context("unknown or blocked provider")?;
    ensure!(
        !require_follow || row.try_get::<bool>("", "is_followed")?,
        "catalogue is not followed"
    );
    ensure!(
        !crate::peer_policy::get(tx, provider).await?.banned,
        "blocked provider"
    );
    Ok(row.try_get("", "applied_cursor")?)
}

/// Called locally for a snapshot negotiated with this provider; the same ID
/// resumes after interruption. A different accepted snapshot replaces staging.
pub async fn begin(
    db: &DatabaseConnection,
    provider: EndpointId,
    snapshot: Uuid,
    cut: i64,
) -> Result<i32> {
    ensure!(
        cut >= 0 && !snapshot.is_nil(),
        "invalid inventory checkpoint"
    );
    let id = provider.to_string();
    let tx = db.begin().await?;
    let applied = lock_policy(&tx, &id, true).await?;
    ensure!(cut >= applied, "obsolete inventory");
    let current = tx
        .query_one(sql(
            "SELECT snapshot,cut,next_page FROM federation_inventories WHERE provider=$1",
            [id.clone().into()],
        ))
        .await?;
    if let Some(row) = current {
        let old_cut: i64 = row.try_get("", "cut")?;
        if row.try_get::<Uuid>("", "snapshot")? == snapshot {
            ensure!(old_cut == cut, "snapshot cut changed");
            let next = row.try_get("", "next_page")?;
            tx.commit().await?;
            return Ok(next);
        }
        ensure!(cut >= old_cut, "obsolete inventory");
        tx.execute(sql(
            "DELETE FROM federation_inventories WHERE provider=$1",
            [id.clone().into()],
        ))
        .await?;
    }
    tx.execute(sql(
        "INSERT INTO federation_inventories(provider,snapshot,cut) VALUES($1,$2,$3)",
        [id.into(), snapshot.into(), cut.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(0)
}

async fn import(tx: &DatabaseTransaction, publication: &SignedPublication) -> Result<Option<Uuid>> {
    let m = &publication.manifest;
    let media = m.media.as_ref().map(serde_json::to_value).transpose()?;
    let row = tx
        .query_one(sql(
            "SELECT federation_import_signed($1,$2,$3,$4,$5) AS track_id",
            [
                m.origin.to_string().into(),
                m.publication.into(),
                m.revision.into(),
                media.into(),
                serde_json::to_value(publication)?.into(),
            ],
        ))
        .await?
        .context("missing publication import result")?;
    Ok(row.try_get("", "track_id")?)
}

/// In-process readers register before their DB admission checks. Notify them
/// after commit; aborted imports never cancel unaffected playback.
async fn revoked_hashes(tx: &DatabaseTransaction, track: Uuid) -> Result<Vec<String>> {
    Ok(tx.query_all(sql("SELECT DISTINCT content_hash FROM publication_versions WHERE track_id=$1 AND content_hash IS NOT NULL",
        [track.into()])).await?.into_iter().map(|r| r.try_get("","content_hash")).collect::<Result<_,_>>()?)
}

fn notify_withdrawal(track: Uuid, hashes: Vec<String>) {
    soundtime_db::media_access::revoke_track(track);
    for hash in hashes {
        soundtime_db::media_access::revoke_legacy_hash(&hash);
    }
}

impl Entry {
    fn verify(&self) -> Result<()> {
        self.publication.verify()?;
        ensure!(
            self.offer_until.is_none() || self.publication.manifest.media.is_some(),
            "withdrawal with a provider offer"
        );
        ensure!(
            self.offer_until
                .is_none_or(|t| t <= chrono::Utc::now().timestamp() + MAX_LEASE_SECS),
            "excessive provider lease"
        );
        Ok(())
    }
}

enum Applied {
    None,
    Withdraw(Uuid, Vec<String>),
    Offer(Uuid, i64),
}

async fn apply_entry(tx: &DatabaseTransaction, provider: &str, entry: &Entry) -> Result<Applied> {
    let manifest = &entry.publication.manifest;
    let Some(track) = import(tx, &entry.publication).await? else {
        return Ok(Applied::None);
    };
    if manifest.media.is_none() {
        return Ok(Applied::Withdraw(track, revoked_hashes(tx, track).await?));
    }
    let Some(until) = entry
        .offer_until
        .filter(|t| *t > chrono::Utc::now().timestamp())
    else {
        return Ok(Applied::None);
    };
    tx.execute(sql(
        "INSERT INTO publication_offers(track_id,revision,provider_node_id,expires_at)
        VALUES($1,$2,$3,to_timestamp($4::bigint)) ON CONFLICT(track_id,revision,provider_node_id)
        DO UPDATE SET expires_at=excluded.expires_at",
        [
            track.into(),
            manifest.revision.into(),
            provider.into(),
            until.into(),
        ],
    ))
    .await?;
    Ok(Applied::Offer(track, manifest.revision))
}

/// Apply a page and its checkpoint as one commit. The final page (including an
/// empty first/final page) removes only missing offers from the given provider.
pub async fn apply_page(db: &DatabaseConnection, provider: EndpointId, bytes: &[u8]) -> Result<()> {
    let page = Page::decode(bytes)?;
    let digest = Sha256::digest(serde_json::to_vec(&page)?).to_vec();
    let id = provider.to_string();
    let tx = db.begin().await?;
    lock_policy(&tx, &id, true).await?;
    let row = tx.query_one(sql("SELECT next_page,complete FROM federation_inventories WHERE provider=$1 AND snapshot=$2 AND cut=$3",
        [id.clone().into(),page.snapshot.into(),page.cut.into()])).await?.context("unrequested inventory")?;
    let next: i32 = row.try_get("", "next_page")?;
    if page.number < next {
        let duplicate = tx.query_one(sql("SELECT 1 FROM federation_inventory_pages WHERE provider=$1 AND page=$2 AND digest=$3",
            [id.into(),page.number.into(),digest.into()])).await?.is_some();
        ensure!(duplicate, "conflicting duplicate page");
        tx.commit().await?;
        return Ok(());
    }
    ensure!(
        page.number == next && !row.try_get::<bool>("", "complete")?,
        "unexpected inventory page"
    );
    let last_publication = page
        .entries
        .last()
        .map(|entry| entry.publication.manifest.publication);
    let mut events = page.entries;
    events.sort_by_key(|e| {
        (
            e.publication.manifest.origin,
            e.publication.manifest.publication,
            e.publication.manifest.revision,
        )
    });
    let mut revoked = Vec::new();
    for entry in events {
        match apply_entry(&tx, &id, &entry).await? {
            Applied::Withdraw(track, hashes) => revoked.push((track, hashes)),
            Applied::Offer(track, revision) => {
                tx.execute(sql("INSERT INTO federation_inventory_seen(provider,track_id,revision) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
                    [id.clone().into(),track.into(),revision.into()])).await?;
            }
            Applied::None => {}
        }
    }
    tx.execute(sql(
        "INSERT INTO federation_inventory_pages(provider,page,digest) VALUES($1,$2,$3)",
        [id.clone().into(), page.number.into(), digest.into()],
    ))
    .await?;
    tx.execute(sql(
        "UPDATE federation_inventories SET next_page=next_page+1,complete=$2,
         last_publication=coalesce($3,last_publication) WHERE provider=$1",
        [id.clone().into(), page.last.into(), last_publication.into()],
    ))
    .await?;
    if page.last {
        tx.execute(sql("DELETE FROM publication_offers o WHERE o.provider_node_id=$1 AND NOT EXISTS
             (SELECT FROM federation_inventory_seen s WHERE s.provider=$1 AND s.track_id=o.track_id AND s.revision=o.revision)",
             [id.clone().into()])).await?;
        tx.execute(sql(
            "DELETE FROM federation_inventory_seen WHERE provider=$1",
            [id.clone().into()],
        ))
        .await?;
        tx.execute(sql(
            "UPDATE federation_peer_policies SET applied_cursor=$2 WHERE node_id=$1",
            [id.into(), page.cut.into()],
        ))
        .await?;
    }
    tx.commit().await?;
    for (track, hashes) in revoked {
        notify_withdrawal(track, hashes);
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub cursor: i64,
    pub change: Change,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Change {
    Put(Box<Entry>),
    RemoveOffer {
        origin: EndpointId,
        publication: Uuid,
        revision: i64,
    },
}

/// Senders order deltas by commit. A replay is harmless, a gap or unfinished
/// snapshot requests recovery and never advances the local checkpoint.
pub async fn apply_delta(
    db: &DatabaseConnection,
    provider: EndpointId,
    bytes: &[u8],
) -> Result<()> {
    ensure!(bytes.len() <= PAGE_BYTES, "delta too large");
    let delta: Delta = serde_json::from_slice(bytes)?;
    ensure!(delta.cursor > 0, "invalid change cursor");
    if let Change::Put(entry) = &delta.change {
        entry.verify()?;
    }
    let id = provider.to_string();
    let tx = db.begin().await?;
    let applied = lock_policy(&tx, &id, true).await?;
    if delta.cursor <= applied {
        tx.commit().await?;
        return Ok(());
    }
    ensure!(
        applied.checked_add(1) == Some(delta.cursor),
        "catalogue change gap"
    );
    ensure!(
        tx.query_one(sql(
            "SELECT 1 FROM federation_inventories WHERE provider=$1 AND complete",
            [id.clone().into()]
        ))
        .await?
        .is_some(),
        "complete the snapshot before deltas"
    );
    let changed = match delta.change {
        Change::Put(entry) => apply_entry(&tx, &id, &entry).await?,
        Change::RemoveOffer {
            origin,
            publication,
            revision,
        } => {
            ensure!(
                revision > 0 && !publication.is_nil(),
                "invalid offer identity"
            );
            tx.execute(sql("DELETE FROM publication_offers o USING track_publications p, federation_catalogs c
                WHERE o.provider_node_id=$1 AND o.revision<=$2 AND o.track_id=p.track_id AND p.catalog_id=c.id
                AND c.node_id=$3 AND p.origin_publication_id=$4",
                [id.clone().into(),revision.into(),origin.to_string().into(),publication.into()])).await?;
            Applied::None
        }
    };
    tx.execute(sql(
        "UPDATE federation_peer_policies SET applied_cursor=$2 WHERE node_id=$1",
        [id.into(), delta.cursor.into()],
    ))
    .await?;
    tx.commit().await?;
    if let Applied::Withdraw(track, hashes) = changed {
        notify_withdrawal(track, hashes);
    }
    Ok(())
}

/// A reachable, locally allowed relay can deliver the origin's terminal decision
/// even after we unfollow its catalogue, but only for a publication already known.
pub async fn apply_withdrawal(
    db: &DatabaseConnection,
    sender: EndpointId,
    bytes: &[u8],
) -> Result<()> {
    ensure!(bytes.len() <= 2048, "withdrawal too large");
    let signed: SignedPublication = serde_json::from_slice(bytes)?;
    signed.verify()?;
    let m = &signed.manifest;
    ensure!(m.media.is_none(), "expected a signed withdrawal");
    let tx = db.begin().await?;
    lock_policy(&tx, &sender.to_string(), false).await?;
    let known = tx
        .query_one(sql(
            "SELECT 1 FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
        WHERE NOT c.is_local AND c.node_id=$1 AND p.origin_publication_id=$2",
            [m.origin.to_string().into(), m.publication.into()],
        ))
        .await?;
    if known.is_none() {
        bail!("unrelated withdrawal");
    }
    let track = import(&tx, &signed).await?;
    let revoked = match track {
        Some(track) => Some((track, revoked_hashes(&tx, track).await?)),
        None => None,
    };
    tx.commit().await?;
    if let Some((track, hashes)) = revoked {
        notify_withdrawal(track, hashes);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
