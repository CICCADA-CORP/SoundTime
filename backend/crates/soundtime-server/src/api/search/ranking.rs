//! Share only the bounded corpus ranking. Every caller resolves its own live page.
use super::NameMatches;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, FromQueryResult, QueryResult, Statement};
use soundtime_db::AppState;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::OnceCell;
use uuid::Uuid;

const PREFIX: usize = 64;
const MAX_ENTRIES: usize = 512;
const TTL: Duration = Duration::from_secs(8);
const PAGE: &str = concat!(
    "WITH names AS MATERIALIZED (",
    include_str!("name_matches.sql"),
    "),",
    include_str!("ranked_page.sql")
);

struct Prefix {
    snapshot: String,
    ids: serde_json::Value,
    complete: bool,
}

struct Entry {
    // A weak reference also prevents pointer reuse for another database.
    owner: Weak<AppState>,
    started: Instant,
    prefix: OnceCell<Option<Prefix>>,
}

type Key = (usize, String);
static CACHE: LazyLock<Mutex<HashMap<Key, Arc<Entry>>>> = LazyLock::new(Default::default);

fn entry(state: &Arc<AppState>, key: &Key) -> Option<Arc<Entry>> {
    let mut cache = CACHE.lock().expect("search ranking cache");
    if let Some(entry) = cache.get(key) {
        let loading = entry.prefix.get().is_none() && Arc::strong_count(entry) > 1;
        if loading || entry.started.elapsed() < TTL {
            return Some(entry.clone());
        }
    }
    cache.remove(key);
    if cache.len() >= MAX_ENTRIES {
        // Never evict a calculation while clients are waiting for it. When the
        // entire cache is busy a new term uses the ordinary database search.
        let oldest = cache
            .iter()
            .filter(|(_, e)| Arc::strong_count(e) == 1)
            .min_by_key(|(_, e)| (e.owner.strong_count() != 0, e.started))
            .map(|(k, _)| k.clone())?;
        cache.remove(&oldest);
    }
    let slot = Arc::new(Entry {
        owner: Arc::downgrade(state),
        started: Instant::now(),
        prefix: OnceCell::new(),
    });
    cache.insert(key.clone(), slot.clone());
    Some(slot)
}

async fn rank(state: &AppState, terms: &str) -> Result<Option<Prefix>, DbErr> {
    let Some(row) = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            include_str!("rank_prefix.sql"),
            [terms.into(), ((PREFIX + 1) as i64).into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let raw = row.try_get::<serde_json::Value>("", "ids")?;
    let mut ids: Vec<Uuid> = serde_json::from_value(raw).map_err(|e| DbErr::Type(e.to_string()))?;
    let complete = ids.len() <= PREFIX;
    ids.truncate(PREFIX);
    Ok(Some(Prefix {
        snapshot: row.try_get("", "snapshot")?,
        ids: serde_json::json!(ids),
        complete,
    }))
}

pub(super) async fn page(
    state: &Arc<AppState>,
    terms: &str,
    limit: i64,
    offset: i64,
    [strict, enabled]: [bool; 2],
) -> Result<Option<(Vec<QueryResult>, NameMatches)>, DbErr> {
    if offset + limit > PREFIX as i64 {
        return Ok(None);
    }
    let key = (Arc::as_ptr(state) as usize, terms.to_owned());
    let Some(slot) = entry(state, &key) else {
        return Ok(None);
    };
    let Some(prefix) = slot.prefix.get_or_try_init(|| rank(state, terms)).await? else {
        return Ok(None);
    };
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            PAGE,
            [
                terms.into(),
                prefix.snapshot.as_str().into(),
                prefix.ids.clone().into(),
                limit.into(),
                offset.into(),
                strict.into(),
                enabled.into(),
            ],
        ))
        .await?;
    let first = rows
        .first()
        .ok_or_else(|| DbErr::Custom("missing search page status".into()))?;
    if !first.try_get::<bool>("", "usable")? {
        let mut cache = CACHE.lock().expect("search ranking cache");
        if cache
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &slot))
        {
            cache.remove(&key);
        }
        return Ok(None);
    }
    let names = NameMatches::from_query_result(first, "")?;
    let empty = first.try_get::<Option<Uuid>>("", "id")?.is_none();
    let hits = if empty { Vec::new() } else { rows };
    if !prefix.complete && hits.len() < limit as usize {
        // An eligible replacement may rank below the cached prefix.
        return Ok(None);
    }
    Ok(Some((hits, names)))
}

#[cfg(test)]
mod tests;
