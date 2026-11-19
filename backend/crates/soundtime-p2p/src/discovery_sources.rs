//! Directory rows and LAN announcements are hints. Only authenticated SoundTime
//! presence can enter the registry, and both dial attempts and input are bounded.
use super::{EndpointAddr, EndpointId, P2pMessage, P2pNode, PEX_LIMIT};
use futures_util::{Stream, StreamExt};
use iroh_mdns_address_lookup::DiscoveryEvent;
use rand::seq::IteratorRandom;
use serde::Deserialize;
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_LIST_BYTES: usize = 1024 * 1024;
const PROBES_PER_MINUTE: usize = 32;

#[derive(Default)]
pub(super) struct ProbeBudget(VecDeque<(Instant, EndpointId)>);

impl ProbeBudget {
    fn admit(&mut self, id: EndpointId, now: Instant) -> bool {
        while self
            .0
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= Duration::from_secs(60))
        {
            self.0.pop_front();
        }
        if self.0.len() >= PROBES_PER_MINUTE || self.0.iter().any(|(_, tried)| *tried == id) {
            return false;
        }
        self.0.push_back((now, id));
        true
    }
}

#[derive(Deserialize)]
struct Listing {
    nodes: Vec<ListedNode>,
}
#[derive(Deserialize)]
struct ListedNode {
    is_online: bool,
    p2p_enabled: bool,
    p2p_node_id: Option<String>,
}

async fn listed_peers(
    client: &reqwest::Client,
    url: &str,
    own: EndpointId,
) -> anyhow::Result<Vec<EndpointId>> {
    let mut response = client
        .get(format!("{}/api/nodes", url.trim_end_matches('/')))
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            chunk.len() <= MAX_LIST_BYTES - body.len(),
            "oversized SoundTime listing"
        );
        body.extend_from_slice(&chunk);
    }
    let listing = serde_json::from_slice::<Listing>(&body)?;
    let mut seen = HashSet::from([own]);
    Ok(listing
        .nodes
        .into_iter()
        .filter(|n| n.is_online && n.p2p_enabled)
        .filter_map(|n| n.p2p_node_id?.parse().ok())
        .filter(|id| seen.insert(*id))
        .choose_multiple(&mut rand::rng(), PEX_LIMIT))
}

impl P2pNode {
    /// Public topology views use the same consent, ban and size rules as PEX.
    pub async fn public_peer_sample(&self) -> Vec<crate::PeerInfo> {
        let ids =
            crate::peer_policy::shareable(&self.db, &self.node_id().to_string(), PEX_LIMIT as i64)
                .await
                .unwrap_or_default();
        let mut peers = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(peer) = self.registry.get_peer(&id).await {
                peers.push(peer);
            }
        }
        peers
    }

    pub fn listing_discovery_enabled(&self) -> bool {
        self.explicit_catalog() && self._config.listing_discovery
    }

    pub async fn discover_listing(
        &self,
        client: &reqwest::Client,
        url: &str,
    ) -> anyhow::Result<()> {
        if self.listing_discovery_enabled() {
            for id in listed_peers(client, url, self.node_id()).await? {
                self.verify_candidate(id).await;
            }
        }
        Ok(())
    }

    pub(super) fn discover_lan(
        self: &Arc<Self>,
        mut events: impl Stream<Item = DiscoveryEvent> + Unpin + Send + 'static,
    ) {
        let node = Arc::clone(self);
        let mut shutdown = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let discover = async {
                while let Some(event) = events.next().await {
                    if let DiscoveryEvent::Discovered { endpoint_info, .. } = event {
                        node.verify_candidate(endpoint_info.endpoint_id).await;
                    }
                }
            };
            tokio::select! { _ = discover => {}, _ = shutdown.changed() => {} }
        });
    }

    pub(super) async fn verify_candidate(&self, id: EndpointId) -> bool {
        let key = id.to_string();
        if id == self.node_id()
            || self
                .registry
                .get_peer(&key)
                .await
                .is_some_and(|p| p.is_online)
        {
            return false;
        }
        if !self
            .candidate_probes
            .lock()
            .expect("discovery probe budget")
            .admit(id, Instant::now())
        {
            return false;
        }
        if !crate::peer_policy::get(&self.db, &key)
            .await
            .is_ok_and(|p| !p.banned)
        {
            return false;
        }
        matches!(tokio::time::timeout(Duration::from_secs(5), self.ping_peer(EndpointAddr::new(id))).await,
            Ok(Ok(P2pMessage::Pong { node_id, .. })) if node_id == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn forged_or_repeated_hints_cannot_exhaust_unbounded_probe_slots() {
        let now = Instant::now();
        let id = SecretKey::generate().public();
        let mut budget = ProbeBudget::default();
        assert!(budget.admit(id, now));
        assert!(!budget.admit(id, now));
        for _ in 1..PROBES_PER_MINUTE {
            assert!(budget.admit(SecretKey::generate().public(), now));
        }
        assert!(!budget.admit(SecretKey::generate().public(), now));
        assert!(budget.admit(id, now + Duration::from_secs(60)));
    }

    #[tokio::test]
    async fn official_directory_format_filters_hints_without_submitting_our_identity(
    ) -> anyhow::Result<()> {
        let server = MockServer::start().await;
        let own = SecretKey::generate().public();
        let other = SecretKey::generate().public();
        let row = |id: String, live, enabled| {
            serde_json::json!({
            "domain":"music.example","p2p_enabled":enabled,"p2p_node_id":id,"is_online":live})
        };
        let good = row(other.to_string(), true, true);
        Mock::given(method("GET")).and(path("/api/nodes")).respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"total":7,"nodes":[good.clone(),good,row(own.to_string(),true,true),
                row(SecretKey::generate().public().to_string(),false,true),
                row(SecretKey::generate().public().to_string(),true,false),
                row("invalid".into(),true,true),{"is_online":true,"p2p_enabled":true,"p2p_node_id":null}]})))
            .expect(1).mount(&server).await;
        assert_eq!(
            listed_peers(&reqwest::Client::new(), &server.uri(), own).await?,
            [other]
        );
        let requests = server.received_requests().await.unwrap();
        assert!(requests[0].url.query().is_none() && requests[0].body.is_empty());
        Ok(())
    }
}
