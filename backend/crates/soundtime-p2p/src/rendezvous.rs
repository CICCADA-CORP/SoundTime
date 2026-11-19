//! Expiring, signed registrations for instances that opt into a shared rendezvous.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use iroh::{EndpointId, SecretKey, Signature};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};

const MAX_AGE: i64 = 15 * 60;
const CLOCK_SKEW: i64 = 60;
const MAX_ENTRIES: usize = 4096;
const MAX_PEERS: usize = 8;

#[derive(Serialize, Deserialize)]
pub struct Registration {
    node_id: EndpointId,
    timestamp: i64,
    signature: Signature,
}

impl Registration {
    fn signed_message(node_id: EndpointId, timestamp: i64) -> String {
        format!("soundtime/rendezvous/1/{node_id}/{timestamp}")
    }

    fn new(key: &SecretKey, timestamp: i64) -> Self {
        let node_id = key.public();
        let message = Self::signed_message(node_id, timestamp);
        Self {
            node_id,
            timestamp,
            signature: key.sign(message.as_bytes()),
        }
    }
}

#[derive(Default)]
pub struct Rendezvous {
    // Expiry is relative to the signed timestamp so replays cannot extend a lease.
    members: HashMap<EndpointId, i64>,
}

#[derive(Debug, PartialEq)]
pub enum RegistrationError {
    Invalid,
    Full,
}

impl Rendezvous {
    pub fn register(
        &mut self,
        request: Registration,
        now: i64,
    ) -> Result<Vec<EndpointId>, RegistrationError> {
        let Registration {
            node_id,
            timestamp,
            signature,
        } = request;
        if !(now - MAX_AGE + 1..=now + CLOCK_SKEW).contains(&timestamp)
            || node_id
                .verify(
                    Registration::signed_message(node_id, timestamp).as_bytes(),
                    &signature,
                )
                .is_err()
        {
            return Err(RegistrationError::Invalid);
        }

        self.members.retain(|_, seen| *seen > now - MAX_AGE);
        if self.members.len() >= MAX_ENTRIES && !self.members.contains_key(&node_id) {
            return Err(RegistrationError::Full);
        }
        let seen = self.members.entry(node_id).or_insert(timestamp);
        *seen = (*seen).max(timestamp);
        Ok(self
            .members
            .keys()
            .copied()
            .filter(|id| *id != node_id)
            .choose_multiple(&mut rand::rng(), MAX_PEERS))
    }
}

pub(crate) async fn discover(
    client: &reqwest::Client,
    url: &str,
    key: &SecretKey,
) -> anyhow::Result<Vec<EndpointId>> {
    let mut response = client
        .post(url)
        .timeout(Duration::from_secs(10))
        .json(&Registration::new(key, chrono::Utc::now().timestamp()))
        .send()
        .await?
        .error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len() + chunk.len() <= 4096,
            "oversized rendezvous response"
        );
        body.extend_from_slice(&chunk);
    }
    let ids: Vec<String> = serde_json::from_slice(&body)?;
    let mut seen = HashSet::from([key.public()]);
    Ok(ids
        .into_iter()
        .filter_map(|raw| raw.parse().ok())
        .filter(|id| seen.insert(*id))
        .take(MAX_PEERS)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_nodes_find_each_other_until_registration_expires() {
        let a = SecretKey::from_bytes(&[1; 32]);
        let b = SecretKey::from_bytes(&[2; 32]);
        let mut directory = Rendezvous::default();
        assert_eq!(directory.register(Registration::new(&a, 0), 0), Ok(vec![]));
        assert_eq!(
            directory.register(Registration::new(&b, 2), 2),
            Ok(vec![a.public()])
        );
        assert_eq!(
            directory.register(Registration::new(&a, 300), 300),
            Ok(vec![b.public()])
        );
        assert_eq!(
            directory.register(Registration::new(&a, MAX_AGE + 2), MAX_AGE + 2),
            Ok(vec![])
        );
    }

    #[test]
    fn rejects_forged_future_and_expired_registrations() {
        let a = SecretKey::from_bytes(&[1; 32]);
        let b = SecretKey::from_bytes(&[2; 32]);
        let mut directory = Rendezvous::default();
        let mut forged = Registration::new(&a, 0);
        forged.node_id = b.public();
        assert_eq!(
            directory.register(forged, 0),
            Err(RegistrationError::Invalid)
        );
        assert_eq!(
            directory.register(Registration::new(&a, 0), MAX_AGE),
            Err(RegistrationError::Invalid)
        );
        assert_eq!(
            directory.register(Registration::new(&a, CLOCK_SKEW + 1), 0),
            Err(RegistrationError::Invalid)
        );
        assert!(directory.members.is_empty());
    }

    #[tokio::test]
    async fn client_sends_registration_and_filters_invalid_self_and_duplicate_ids() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let a = SecretKey::from_bytes(&[1; 32]);
        let b = SecretKey::from_bytes(&[2; 32]);
        let server = MockServer::start().await;
        let directory = std::sync::Mutex::new(Rendezvous::default());
        Mock::given(method("POST"))
            .and(path("/api/p2p/rendezvous"))
            .respond_with(move |request: &wiremock::Request| {
                let registration = request.body_json::<Registration>().unwrap();
                let own_id = registration.node_id;
                let peers = directory
                    .lock()
                    .unwrap()
                    .register(registration, chrono::Utc::now().timestamp())
                    .unwrap();
                let mut response = vec!["invalid".to_owned(), own_id.to_string()];
                for peer in peers {
                    response.push(peer.to_string());
                    response.push(peer.to_string());
                }
                ResponseTemplate::new(200).set_body_json(response)
            })
            .expect(3)
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/api/p2p/rendezvous", server.uri());
        assert!(discover(&client, &url, &a).await.unwrap().is_empty());
        assert_eq!(discover(&client, &url, &b).await.unwrap(), vec![a.public()]);
        assert_eq!(discover(&client, &url, &a).await.unwrap(), vec![b.public()]);
    }
}
