use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};
use serde::de::{Deserializer, IgnoredAny, SeqAccess, Visitor};

pub const PEX_LIMIT: usize = 8;

/// Legacy senders may provide their whole registry. Consume the JSON sequence
/// while retaining at most the first bounded sample for verification.
pub fn read_candidates<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<String>, D::Error> {
    struct Sample;
    impl<'de> Visitor<'de> for Sample {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of endpoint identities")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut list: A) -> Result<Self::Value, A::Error> {
            let mut peers = Vec::with_capacity(PEX_LIMIT);
            for _ in 0..PEX_LIMIT {
                let Some(id) = list.next_element::<String>()? else {
                    return Ok(peers);
                };
                if id.len() <= 128 {
                    peers.push(id);
                }
            }
            while list.next_element::<IgnoredAny>()?.is_some() {}
            Ok(peers)
        }
    }
    input.deserialize_seq(Sample)
}

pub struct Policy {
    pub followed: bool,
    pub banned: bool,
}

pub async fn get(db: &impl ConnectionTrait, id: &str) -> Result<Policy, DbErr> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT COALESCE((SELECT is_followed FROM federation_peer_policies WHERE node_id=$1), false) AS followed,
                COALESCE((SELECT is_banned FROM federation_peer_policies WHERE node_id=$1), false)
                OR EXISTS (SELECT 1 FROM blocked_domains WHERE domain=$1) AS banned", [id.into()]))
        .await?.ok_or_else(|| DbErr::Custom("missing peer policy result".into()))?;
    Ok(Policy {
        followed: row.try_get("", "followed")?,
        banned: row.try_get("", "banned")?,
    })
}

pub async fn follow(db: &impl ConnectionTrait, id: &str, followed: bool) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO federation_peer_policies(node_id,is_followed) VALUES ($1,$2)
         ON CONFLICT(node_id) DO UPDATE SET
           source_disabled=CASE WHEN excluded.is_followed THEN false
                               ELSE federation_peer_policies.is_followed OR federation_peer_policies.source_disabled END,
           is_followed=excluded.is_followed",
        [id.into(), followed.into()],
    ))
    .await?;
    Ok(())
}

pub async fn disconnect(db: &impl ConnectionTrait, id: &str) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_peer_policies(node_id,is_followed,source_disabled) VALUES($1,false,true)
         ON CONFLICT(node_id) DO UPDATE SET is_followed=false,source_disabled=true",[id.into()])).await?;
    Ok(())
}

/// Public federation serves known identities unless blocked. A private operator
/// chooses their federation counterpart by following it locally.
pub async fn permits_export(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
    let row=db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT NOT EXISTS(SELECT FROM federation_peer_policies WHERE node_id=$1 AND is_banned)
         AND NOT EXISTS(SELECT FROM blocked_domains WHERE domain=$1)
         AND (NOT EXISTS(SELECT FROM instance_settings WHERE key='instance_private' AND value='true')
              OR EXISTS(SELECT FROM federation_peer_policies WHERE node_id=$1 AND is_followed)) AS allowed",
        [id.into()])).await?.ok_or_else(||DbErr::Custom("missing federation export policy".into()))?;
    row.try_get("", "allowed")
}

/// Only the authenticated owner of this identity supplies this value.
pub async fn record_consent(db: &impl ConnectionTrait, id: &str, share: bool) -> Result<(), DbErr> {
    if !share {
        db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE federation_peer_policies SET share_identity=false WHERE node_id=$1 AND share_identity", [id.into()])).await?;
        return Ok(());
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO federation_peer_policies(node_id,share_identity) VALUES ($1,$2)
         ON CONFLICT(node_id) DO UPDATE SET share_identity=excluded.share_identity
         WHERE federation_peer_policies.share_identity IS DISTINCT FROM excluded.share_identity",
        [id.into(), share.into()],
    ))
    .await?;
    Ok(())
}

pub async fn shareable(
    db: &impl ConnectionTrait,
    recipient: &str,
    limit: i64,
) -> Result<Vec<String>, DbErr> {
    // Pick a point on the keyspace and wrap once. Both ranges use the index;
    // an instance with hundreds of subscriptions can expose different samples.
    let pivot=data_encoding::HEXLOWER.encode(&rand::random::<[u8;32]>());
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "WITH eligible AS NOT MATERIALIZED (SELECT p.node_id FROM federation_peer_policies p
         WHERE p.share_identity AND NOT p.is_banned AND p.node_id <> $1
           AND NOT EXISTS (SELECT 1 FROM blocked_domains b WHERE b.domain=p.node_id))
         SELECT node_id FROM (
           (SELECT node_id,0 AS part FROM eligible WHERE node_id >= $3 ORDER BY node_id LIMIT $2)
           UNION ALL
           (SELECT node_id,1 AS part FROM eligible WHERE node_id < $3 ORDER BY node_id LIMIT $2)
         ) sampled ORDER BY part,node_id LIMIT $2",
        [recipient.into(), limit.into(),pivot.into()],
    ))
    .await?
    .iter()
    .map(|row| row.try_get("", "node_id"))
    .collect()
}

pub async fn followed_after(db: &impl ConnectionTrait, after: &str) -> Result<Vec<String>, DbErr> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT p.node_id FROM federation_peer_policies p
         WHERE p.node_id > $1 AND p.is_followed AND NOT p.is_banned AND NOT EXISTS
         (SELECT 1 FROM blocked_domains b WHERE b.domain=p.node_id)
         ORDER BY p.node_id LIMIT 8",
        [after.into()],
    ))
    .await?
    .iter()
    .map(|row| row.try_get("", "node_id"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, TransactionTrait};

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL for PostgreSQL"]
    async fn pex_consent_and_subscription_with_postgres() -> anyhow::Result<()> {
        let db = Database::connect(std::env::var("TEST_DATABASE_URL")?).await?;
        let txn = db.begin().await?;
        txn.execute_unprepared(
            "CREATE TEMP TABLE federation_peer_policies(node_id text PRIMARY KEY, is_followed boolean DEFAULT false, is_banned boolean DEFAULT false, share_identity boolean DEFAULT false, source_disabled boolean DEFAULT false);
             CREATE TEMP TABLE blocked_domains(domain text PRIMARY KEY);
             INSERT INTO blocked_domains VALUES ('blocked-by-admin')"
        ).await?;
        for id in [
            "visible",
            "requester",
            "blocked-by-admin",
            "banned",
            "opted-out",
        ] {
            record_consent(&txn, id, true).await?;
        }
        txn.execute_unprepared(
            "UPDATE federation_peer_policies SET is_banned=true WHERE node_id='banned'",
        )
        .await?;
        record_consent(&txn, "opted-out", false).await?;
        follow(&txn, "private-followed", true).await?;
        assert_eq!(shareable(&txn, "requester", 8).await?, vec!["visible"]);
        assert!(get(&txn, "blocked-by-admin").await?.banned);
        assert!(get(&txn, "private-followed").await?.followed);
        follow(&txn, "private-followed", false).await?;
        assert!(!get(&txn, "private-followed").await?.followed);
        txn.rollback().await?;
        Ok(())
    }
}
