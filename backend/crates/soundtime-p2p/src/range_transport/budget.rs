//! Rate limits for the single owner of a peer identity. One window may burst.
use anyhow::{ensure, Result};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(super) fn setting(name: &str, default: u64, max: u64) -> u64 {
    std::env::var(name).map_or(default, |s| {
        s.parse::<u64>()
            .ok()
            .filter(|n| *n <= max)
            .unwrap_or_else(|| panic!("{name} must be between zero and {max}"))
    })
}

const BURST: f64 = 128.0 * 1024.0;
struct Bucket {
    tokens: f64,
    updated: Instant,
}
impl Bucket {
    fn new(now: Instant) -> Self {
        Self {
            tokens: BURST,
            updated: now,
        }
    }
    fn replenish(&mut self, now: Instant, rate: u64) {
        self.tokens =
            (self.tokens + now.duration_since(self.updated).as_secs_f64() * rate as f64).min(BURST);
        self.updated = now;
    }
    fn delay(&self, bytes: usize, rate: u64) -> f64 {
        if rate == 0 {
            0.0
        } else {
            (bytes as f64 - self.tokens).max(0.0) / rate as f64
        }
    }
}
pub(super) struct Budget {
    global: u64,
    per_peer: u64,
    state: Mutex<(Bucket, HashMap<EndpointId, Bucket>)>,
}

impl Budget {
    pub(super) fn new(global: &str, peer: &str) -> Self {
        Self {
            global: setting(global, 0, 1 << 40),
            per_peer: setting(peer, 0, 1 << 40),
            state: Mutex::new((Bucket::new(Instant::now()), HashMap::new())),
        }
    }
    pub(super) async fn admit(&self, peer: EndpointId, bytes: usize) -> Result<()> {
        if self.global == 0 && self.per_peer == 0 {
            return Ok(());
        }
        ensure!(bytes as f64 <= BURST, "range exceeds bandwidth burst");
        loop {
            let delay = {
                let mut state = self.state.lock().expect("range bandwidth");
                let now = Instant::now();
                let (total, peers) = &mut *state;
                total.replenish(now, self.global);
                let mut delay = total.delay(bytes, self.global);
                if self.per_peer != 0 {
                    if peers.len() >= 4096 && !peers.contains_key(&peer) {
                        peers.retain(|_, bucket| {
                            bucket.replenish(now, self.per_peer);
                            bucket.tokens < BURST
                        });
                        ensure!(peers.len() < 4096, "per-peer bandwidth ledger is full");
                    }
                    let account = peers.entry(peer).or_insert_with(|| Bucket::new(now));
                    account.replenish(now, self.per_peer);
                    delay = delay.max(account.delay(bytes, self.per_peer));
                    if delay == 0.0 {
                        account.tokens -= bytes as f64;
                    }
                }
                if delay == 0.0 {
                    if self.global != 0 {
                        total.tokens -= bytes as f64;
                    }
                    return Ok(());
                }
                delay
            };
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refill_is_bounded_and_retains_debt() {
        let start = Instant::now();
        let mut b = Bucket::new(start);
        b.tokens = 0.0;
        assert_eq!(b.delay(2048, 1024), 2.0);
        b.replenish(start + Duration::from_secs(1), 1024);
        assert_eq!(b.delay(2048, 1024), 1.0);
        b.replenish(start + Duration::from_secs(1000), 1024);
        assert_eq!(b.tokens, BURST);
    }
}
