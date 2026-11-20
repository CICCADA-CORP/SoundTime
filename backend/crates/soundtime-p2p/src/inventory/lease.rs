use super::*;

pub const LEASE_SECONDS: i64 = 3600;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub provider: EndpointId,
    pub origin: EndpointId,
    pub publication: Uuid,
    pub revision: i64,
    pub hash: String,
    pub size: i64,
    pub generation: i64,
    pub expires: i64,
    pub active: bool,
}

impl Lease {
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = b"soundtime/mirror-lease/1\0".to_vec();
        serde_json::to_writer(&mut bytes, self)?;
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLease {
    pub lease: Lease,
    pub signature: Signature,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub provider: SignedLease,
    pub publication: SignedPublication,
}

impl Offer {
    pub(crate) fn issue(
        key: &SecretKey,
        publication: SignedPublication,
        generation: i64,
        active: bool,
    ) -> Result<Self> {
        let m = &publication.manifest;
        let media = m.media.as_ref().context("withdrawn publication")?;
        let lease = Lease {
            provider: key.public(),
            origin: m.origin,
            publication: m.publication,
            revision: m.revision,
            hash: media.hash.clone(),
            size: media.size,
            generation,
            active,
            expires: chrono::Utc::now().timestamp() + LEASE_SECONDS,
        };
        let signature = key.sign(&lease.signing_bytes()?);
        Ok(Self {
            provider: SignedLease { lease, signature },
            publication,
        })
    }

    pub(crate) fn verify(&self, sender: EndpointId) -> Result<()> {
        let l = &self.provider.lease;
        ensure!(
            sender == l.provider || sender == l.origin,
            "unrelated offer relay"
        );
        ensure!(
            l.origin != l.provider
                && l.generation > 0
                && l.expires > 0
                && l.expires <= chrono::Utc::now().timestamp() + LEASE_SECONDS + 60,
            "invalid mirror lease"
        );
        let m = &self.publication.manifest;
        let media = m
            .media
            .as_ref()
            .context("withdrawn publication cannot be offered")?;
        ensure!(
            (l.origin, l.publication, l.revision, l.size, &l.hash)
                == (m.origin, m.publication, m.revision, media.size, &media.hash),
            "lease describes another publication or asset"
        );
        self.publication.verify()?;
        l.provider
            .verify(&l.signing_bytes()?, &self.provider.signature)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JournalEntry {
    pub cursor: i64,
    pub offer: Offer,
}

pub(crate) async fn current(db: &DatabaseConnection, after: i64) -> Result<Vec<JournalEntry>> {
    ensure!(after >= 0, "invalid mirror cursor");
    let rows = db
        .query_all(sql(
            "SELECT l.relay_cursor,l.envelope,l.manifest FROM federation_mirror_leases l
        WHERE l.relay_cursor>$1 AND NOT EXISTS(SELECT FROM blocked_domains b WHERE b.domain=l.provider)
        AND NOT EXISTS(SELECT FROM federation_peer_policies p WHERE p.node_id=l.provider AND (p.is_banned OR p.source_disabled))
        ORDER BY l.relay_cursor LIMIT 16",
            [after.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(JournalEntry {
                cursor: r.try_get("", "relay_cursor")?,
                offer: Offer {
                    provider: serde_json::from_value(r.try_get("", "envelope")?)?,
                    publication: serde_json::from_value(r.try_get("", "manifest")?)?,
                },
            })
        })
        .collect()
}

/// Save only increasing provider generations. The receiver can relay proof for
/// its own publications or its own copies. Stored revisions retain the signed
/// eviction, so a late renewal from before the eviction cannot reopen it.
pub(crate) async fn accept(
    db: &DatabaseConnection,
    local: EndpointId,
    sender: EndpointId,
    offer: &Offer,
    journal: Option<i64>,
) -> Result<bool> {
    offer.verify(sender)?;
    let tx = db.begin().await?;
    let l = &offer.provider.lease;
    let sender_id = sender.to_string();
    let provider_id = l.provider.to_string();
    let origin_id = l.origin.to_string();
    // Obtain locks on the provider and relay in a stable order, including new
    // identities. Registration alone grants no follow or PEX consent.
    let mut ids = vec![sender_id.clone(), provider_id.clone()];
    ids.sort();
    ids.dedup();
    for id in &ids {
        tx.execute(sql(
            "INSERT INTO federation_peer_policies(node_id) VALUES($1) ON CONFLICT DO NOTHING",
            [id.as_str().into()],
        ))
        .await?;
        tx.query_one(sql(
            "SELECT 1 FROM federation_peer_policies WHERE node_id=$1 FOR UPDATE",
            [id.as_str().into()],
        ))
        .await?;
    }
    let sender_policy = crate::peer_policy::get(&tx, &sender_id).await?;
    ensure!(!sender_policy.banned, "blocked offer sender");
    if let Some(cursor) = journal {
        ensure!(
            sender_policy.followed && cursor > 0,
            "unrequested mirror journal"
        );
        // A repeated batch never moves its durable checkpoint backwards.
        tx.execute(sql("UPDATE federation_peer_policies SET mirror_cursor=GREATEST(mirror_cursor,$2) WHERE node_id=$1",
            [sender_id.clone().into(),cursor.into()])).await?;
    }
    let provider = crate::peer_policy::get(&tx, &provider_id).await?;
    let origin = crate::peer_policy::get(&tx, &origin_id).await?;
    let disabled = tx
        .query_one(sql(
            "SELECT 1 FROM federation_peer_policies WHERE node_id=$1 AND source_disabled",
            [provider_id.clone().into()],
        ))
        .await?
        .is_some();
    if l.provider == local
        || provider.banned
        || disabled
        || !(sender_policy.followed || origin.followed || provider.followed || l.origin == local)
    {
        tx.commit().await?;
        return Ok(false);
    }
    let known=tx.query_one(sql("SELECT p.track_id,p.revision,p.state FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
        WHERE c.node_id=$1 AND p.origin_publication_id=$2",[origin_id.clone().into(),l.publication.into()])).await?;
    if let Some(p) = known {
        if p.try_get::<String>("", "state")? == "withdrawn"
            || p.try_get::<i64>("", "revision")? > l.revision
        {
            tx.commit().await?;
            return Ok(false);
        }
    } else if (!sender_policy.followed && !provider.followed)
        || !l.active
        || l.expires <= chrono::Utc::now().timestamp()
    {
        tx.commit().await?;
        return Ok(false);
    }
    let Some(track) = import(&tx, &offer.publication).await? else {
        tx.commit().await?;
        return Ok(false);
    };
    let exported = l.provider == local || l.origin == local;
    let changed = store(&tx, track, offer, exported).await?;
    tx.commit().await?;
    Ok(changed)
}

pub(crate) async fn tick(tx: &DatabaseTransaction) -> Result<i64> {
    let row = tx
        .query_one(sql(
            "UPDATE federation_mirror_clock SET value=value+1 WHERE singleton RETURNING value",
            [],
        ))
        .await?
        .context("missing mirror clock")?;
    Ok(row.try_get("", "value")?)
}

/// Keep up to two known routes for a still-retained publication, even when a
/// listener was offline longer than the lease lifetime. Expired leases grant
/// no media access. Withdrawn or unreferenced publications need no such route.
pub(crate) async fn prune(db: &DatabaseConnection) -> Result<()> {
    db.execute(sql(
        "DELETE FROM federation_mirror_leases del USING (
          SELECT l.track_id,l.revision,l.provider FROM federation_mirror_leases l
          WHERE l.expires_at < now()-interval '2 hours' AND NOT EXISTS(
            SELECT FROM track_publications p WHERE p.track_id=l.track_id AND p.revision=l.revision AND p.state='active'
              AND EXISTS(SELECT FROM federation_withdrawal_references refs WHERE refs.track_id=p.track_id)
              AND l.provider IN (
                SELECT candidate.provider FROM federation_mirror_leases candidate
                WHERE candidate.track_id=l.track_id AND candidate.revision=l.revision
                  AND candidate.provider IS DISTINCT FROM (SELECT node_id FROM federation_catalogs WHERE is_local)
                  AND NOT EXISTS(SELECT FROM blocked_domains b WHERE b.domain=candidate.provider)
                  AND NOT EXISTS(SELECT FROM federation_peer_policies peer WHERE peer.node_id=candidate.provider AND peer.is_banned)
                ORDER BY candidate.expires_at DESC,candidate.provider LIMIT 2))
          ORDER BY l.expires_at LIMIT 128) old
        WHERE (del.track_id,del.revision,del.provider)=(old.track_id,old.revision,old.provider)",
        [],
    )).await?;
    Ok(())
}

pub(crate) async fn store(
    tx: &DatabaseTransaction,
    track: Uuid,
    offer: &Offer,
    exported: bool,
) -> Result<bool> {
    let l = &offer.provider.lease;
    let provider = l.provider.to_string();
    let envelope = serde_json::to_value(&offer.provider)?;
    let prior=tx.query_one(sql("SELECT generation,envelope FROM federation_mirror_leases WHERE track_id=$1 AND revision=$2 AND provider=$3 FOR UPDATE",
        [track.into(),l.revision.into(),provider.clone().into()])).await?;
    if let Some(row) = prior {
        let generation: i64 = row.try_get("", "generation")?;
        if generation == l.generation {
            ensure!(
                row.try_get::<serde_json::Value>("", "envelope")? == envelope,
                "conflicting mirror generation"
            );
        }
        if generation >= l.generation {
            return Ok(false);
        }
    } else {
        // Bound untrusted provider cardinality per already authorized publication.
        let row=tx.query_one(sql("SELECT count(*) AS n FROM federation_mirror_leases WHERE track_id=$1 AND revision=$2 AND expires_at>now()",
            [track.into(),l.revision.into()])).await?.context("missing lease count")?;
        if row.try_get::<i64>("", "n")? >= 32 || l.expires <= chrono::Utc::now().timestamp() {
            return Ok(false);
        }
    }
    let relay_cursor = if exported {
        Some(tick(tx).await?)
    } else {
        None
    };
    tx.execute(sql("INSERT INTO federation_mirror_leases(track_id,revision,provider,generation,expires_at,active,envelope,manifest,relay_cursor)
        VALUES($1,$2,$3,$4,to_timestamp($5::bigint),$6,$7,$8,$9)
        ON CONFLICT(track_id,revision,provider) DO UPDATE SET generation=excluded.generation,expires_at=excluded.expires_at,
        active=excluded.active,envelope=excluded.envelope,manifest=excluded.manifest,relay_cursor=excluded.relay_cursor",
        [track.into(),l.revision.into(),provider.into(),l.generation.into(),l.expires.into(),l.active.into(),envelope.into(),
         serde_json::to_value(&offer.publication)?.into(),relay_cursor.into()])).await?;
    Ok(true)
}
