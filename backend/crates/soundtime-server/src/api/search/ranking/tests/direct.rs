use super::super::{DIRECT, PAGE};
use anyhow::{ensure, Context, Result};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement,
    TransactionTrait,
};
use uuid::Uuid;

const FIRST: Uuid = Uuid::from_u128(101);
const SECOND: Uuid = Uuid::from_u128(102);
const TERM: &str = "Prooffixture:*";

struct Corpus {
    snapshot: String,
    ids: serde_json::Value,
}

impl Corpus {
    async fn check(
        &self,
        tx: &DatabaseTransaction,
        offset: i64,
        strict: bool,
        enabled: bool,
        direct: bool,
        wanted: &[Uuid],
    ) -> Result<()> {
        let statement = |sql| {
            Statement::from_sql_and_values(
                DbBackend::Postgres,
                sql,
                [
                    TERM.into(),
                    self.snapshot.as_str().into(),
                    self.ids.clone().into(),
                    2_i64.into(),
                    offset.into(),
                    strict.into(),
                    enabled.into(),
                ],
            )
        };
        let quick = tx.query_all(statement(DIRECT)).await?;
        let normal = tx.query_all(statement(PAGE)).await?;
        ensure!(
            quick[0].try_get::<bool>("", "usable")?,
            "rank snapshot {} is stale for prefix {}",
            self.snapshot,
            self.ids
        );
        let actual = quick[0].try_get::<bool>("", "direct")?;
        ensure!(
            actual == direct,
            "direct proof was {actual}, expected {direct} (offset {offset}, strict {strict}, P2P {enabled})"
        );
        let identifiers = |rows: &[sea_orm::QueryResult]| -> Result<Vec<Uuid>> {
            let ids: Vec<_> = rows
                .iter()
                .map(|r| r.try_get::<Option<Uuid>>("", "id"))
                .collect::<Result<_, _>>()?;
            Ok(ids.into_iter().flatten().collect())
        };
        ensure!(identifiers(&normal)? == wanted);
        if direct {
            ensure!(identifiers(&quick)? == wanted);
            for (a, b) in quick.iter().zip(normal.iter()) {
                for key in [
                    "result_artist_name",
                    "result_album_title",
                    "result_cover_url",
                ] {
                    ensure!(
                        a.try_get::<Option<String>>("", key)?
                            == b.try_get::<Option<String>>("", key)?
                    );
                }
                ensure!(
                    a.try_get::<Option<i64>>("", "ordinal")?
                        == b.try_get::<Option<i64>>("", "ordinal")?
                );
            }
        }
        Ok(())
    }
}

pub(super) async fn exercise(db: &DatabaseConnection) -> Result<()> {
    // Indexing commits before an HTTP reader can see the new documents.
    let writer = db.begin().await?;
    writer
        .execute_unprepared(include_str!("direct_fixture.sql"))
        .await?;
    writer.commit().await?;

    let tx = db.begin().await?;
    let prefix = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            include_str!("../../rank_prefix.sql"),
            [TERM.into(), 65_i64.into()],
        ))
        .await?
        .context("missing proof prefix")?;
    let corpus = Corpus {
        snapshot: prefix.try_get("", "snapshot")?,
        ids: prefix.try_get("", "ids")?,
    };
    corpus
        .check(&tx, 0, true, true, true, &[FIRST, SECOND])
        .await
        .context("first direct page after indexing committed")?;
    corpus.check(&tx, 1, true, true, true, &[SECOND]).await?;
    corpus.check(&tx, 0, true, false, false, &[]).await?;
    for (label, change) in [
        ("expired","UPDATE publication_offers SET expires_at=now() WHERE track_id='00000000-0000-0000-0000-000000000065'"),
        ("unfollowed","UPDATE federation_peer_policies SET is_followed=false WHERE node_id='proof-source-101'"),
        ("banned","UPDATE federation_peer_policies SET is_banned=true WHERE node_id='proof-source-101'"),
        ("disabled","UPDATE federation_peer_policies SET source_disabled=true WHERE node_id='proof-source-101'"),
        ("offline","INSERT INTO p2p_peers(node_id,is_online) VALUES('proof-source-101',false)"),
        ("blocked domain","INSERT INTO blocked_domains(id,domain) VALUES(gen_random_uuid(),'proof-source-101')"),
        ("withdrawn","UPDATE track_publications SET state='withdrawn' WHERE track_id='00000000-0000-0000-0000-000000000065'"),
        ("detached","UPDATE track_publications SET state='detached' WHERE track_id='00000000-0000-0000-0000-000000000065'"),
        ("hidden","INSERT INTO federation_hidden_publications(track_id) VALUES('00000000-0000-0000-0000-000000000065')"),
        ("blocked hash","INSERT INTO federation_content_blocks(content_hash) VALUES(repeat('a',64))"),
        ("deleted document","DELETE FROM federation_search_documents WHERE track_id='00000000-0000-0000-0000-000000000065'"),
    ] {
        tx.execute_unprepared("SAVEPOINT before_case").await?;
        tx.execute_unprepared(change).await?;
        corpus.check(&tx,0,true,true,false,&[SECOND]).await.context(label)?;
        corpus.check(&tx,1,true,true,false,&[]).await.context(label)?;
        if label=="unfollowed" {corpus.check(&tx,0,false,true,true,&[FIRST,SECOND]).await?;}
        if label=="expired" {
            tx.execute_unprepared("INSERT INTO federation_mirror_leases(track_id,revision,provider,generation,expires_at,active,envelope,manifest) VALUES('00000000-0000-0000-0000-000000000065',1,'proof-mirror',1,now()+interval '1 hour',true,'{}','{}')").await?;
            corpus.check(&tx,0,true,true,false,&[FIRST,SECOND]).await?;
        }
        tx.execute_unprepared("ROLLBACK TO before_case; RELEASE before_case").await?;
    }
    tx.rollback().await?;
    Ok(())
}
