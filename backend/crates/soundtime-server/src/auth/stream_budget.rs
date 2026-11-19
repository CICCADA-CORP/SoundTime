//! Admission for active HTTP audio responses, including time spent resolving a source.
use axum::{
    body::{Body, HttpBody},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::{stream, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Default)]
struct Active {
    total: usize,
    anonymous: usize,
    accounts: HashMap<Uuid, usize>,
}

pub struct StreamBudget {
    total: usize,
    per_account: usize,
    anonymous: usize,
    active: Mutex<Active>,
}

pub(super) struct Permit {
    budget: Arc<StreamBudget>,
    account: Option<Uuid>,
}

impl StreamBudget {
    pub fn configured() -> Arc<Self> {
        fn limit(key: &str, default: usize) -> usize {
            match std::env::var(key) {
                Err(_) => default,
                Ok(raw) => raw
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .unwrap_or_else(|| {
                        tracing::warn!(%key, default,"invalid stream limit; using default");
                        default
                    }),
            }
        }
        Self::new(
            limit("SOUNDTIME_MAX_ACTIVE_STREAMS", 128),
            limit("SOUNDTIME_MAX_STREAMS_PER_ACCOUNT", 8),
            limit("SOUNDTIME_MAX_ANONYMOUS_STREAMS", 32),
        )
    }

    pub(super) fn new(total: usize, per_account: usize, anonymous: usize) -> Arc<Self> {
        Arc::new(Self {
            total,
            per_account,
            anonymous,
            active: Mutex::default(),
        })
    }

    pub(super) fn admit(self: &Arc<Self>, account: Option<Uuid>) -> Result<Permit, StatusCode> {
        let mut active = self.active.lock().expect("active HTTP streams");
        if account
            .is_some_and(|id| active.accounts.get(&id).copied().unwrap_or(0) >= self.per_account)
        {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        if active.total >= self.total || (account.is_none() && active.anonymous >= self.anonymous) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        match account {
            Some(id) => *active.accounts.entry(id).or_default() += 1,
            None => active.anonymous += 1,
        }
        active.total += 1;
        Ok(Permit {
            budget: self.clone(),
            account,
        })
    }
}

pub(super) fn busy(status: StatusCode) -> Response {
    // The response discloses neither the requested track nor a listener's identity.
    (
        status,
        [
            (header::RETRY_AFTER, "1"),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        Json(serde_json::json!({"error":"Audio streaming limit reached"})),
    )
        .into_response()
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut active = self.budget.active.lock().expect("active HTTP streams");
        active.total -= 1;
        match self.account {
            None => active.anonymous -= 1,
            Some(id) => {
                let count = active.accounts.get_mut(&id).expect("admitted account");
                *count -= 1;
                if *count == 0 {
                    active.accounts.remove(&id);
                }
            }
        }
    }
}

impl Permit {
    pub(super) fn hold(self, mut response: Response) -> Response {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        );
        if !response.status().is_success() || response.body().is_end_stream() {
            return response;
        }
        response.map(|body| {
            let stream = stream::try_unfold(
                (body.into_data_stream(), self),
                |(mut body, permit)| async move {
                    Ok::<_, axum::Error>(
                        body.next()
                            .await
                            .transpose()?
                            .map(|data| (data, (body, permit))),
                    )
                },
            );
            Body::from_stream(stream)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use std::io;

    #[test]
    fn separate_accounts_and_anonymous_readers_share_an_instance_limit() {
        let budget = StreamBudget::new(3, 1, 1);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let a = budget.admit(Some(first)).unwrap();
        assert_eq!(
            budget.admit(Some(first)).err(),
            Some(StatusCode::TOO_MANY_REQUESTS)
        );
        let anon = budget.admit(None).unwrap();
        assert_eq!(
            budget.admit(None).err(),
            Some(StatusCode::SERVICE_UNAVAILABLE)
        );
        let b = budget.admit(Some(second)).unwrap();
        assert_eq!(
            budget.admit(Some(Uuid::new_v4())).err(),
            Some(StatusCode::SERVICE_UNAVAILABLE)
        );
        drop((anon, a, b));
        assert!(budget.active.lock().unwrap().accounts.is_empty());
        assert!(budget.admit(Some(second)).is_ok());
    }

    #[tokio::test]
    async fn response_releases_at_eof_error_or_cancellation() {
        let budget = StreamBudget::new(1, 1, 1);
        let id = Some(Uuid::new_v4());
        let body = Body::from_stream(stream::iter([Ok::<_, io::Error>(Bytes::from_static(
            b"part",
        ))]));
        let response = budget.admit(id).unwrap().hold(Response::new(body));
        let mut data = response.into_body().into_data_stream();
        assert!(data.next().await.unwrap().is_ok());
        assert!(budget.admit(id).is_err());
        assert!(data.next().await.is_none());
        assert!(budget.admit(id).is_ok());

        let pending = Body::from_stream(stream::pending::<Result<Bytes, io::Error>>());
        let response = budget.admit(id).unwrap().hold(Response::new(pending));
        assert!(budget.admit(id).is_err());
        drop(response);
        let bad = Body::from_stream(stream::iter([Err::<Bytes, _>(io::Error::other(
            "storage failed",
        ))]));
        let mut data = budget
            .admit(id)
            .unwrap()
            .hold(Response::new(bad))
            .into_body()
            .into_data_stream();
        assert!(data.next().await.unwrap().is_err());
        assert!(budget.admit(id).is_ok());
        let failed = budget
            .admit(id)
            .unwrap()
            .hold(StatusCode::NOT_FOUND.into_response());
        assert!(budget.admit(id).is_ok());
        drop(failed);
    }
}
