use super::{presence_probe, ConnectionPool, P2pError, P2pMessage};
use iroh::{
    endpoint::{Connection, ReadToEndError},
    EndpointId,
};
use std::{future::Future, io, time::Duration};
use tokio::time::{timeout_at, Instant};

/// A dead connection can still accept a local stream and stall before Pong.
/// Only modern Presence is idempotent. Replaying a v1 Ping would start a second catalogue.
pub(super) async fn query<F>(
    pool: &ConnectionPool,
    peer: EndpointId,
    share_identity: bool,
    versioned: bool,
    policy: impl Fn() -> F,
) -> Result<(P2pMessage, P2pMessage), P2pError>
where
    F: Future<Output = Result<Option<bool>, P2pError>>,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut retried = false;
    loop {
        let followed = policy().await?;
        let conn = timeout_at(deadline, pool.get_connection(peer))
            .await
            .map_err(|_| P2pError::Connection("peer presence dial timed out".into()))??;
        let probe = match followed {
            Some(followed) => presence_probe(conn.alpn(), followed, share_identity, versioned)?,
            None => P2pMessage::Ping,
        };
        let may_retry = !retried && matches!(probe, P2pMessage::Presence { .. });
        let until = if may_retry {
            deadline.min(Instant::now() + Duration::from_secs(3))
        } else {
            deadline
        };
        let response = timeout_at(until, exchange(&conn, &probe))
            .await
            .unwrap_or_else(|_| {
                Err(P2pError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "peer presence timed out",
                )))
            });
        match response {
            Ok(reply) => return Ok((probe, reply)),
            Err(error) => {
                pool.invalidate_connection(&conn);
                if may_retry && matches!(error, P2pError::Io(_)) && Instant::now() < deadline {
                    tracing::debug!(%peer, %error, "retrying presence on a fresh control connection");
                    retried = true;
                } else {
                    return Err(error);
                }
            }
        }
    }
}

async fn exchange(conn: &Connection, probe: &P2pMessage) -> Result<P2pMessage, P2pError> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(io::Error::other)?;
    let ping = serde_json::to_vec(probe)?;
    send.write_all(&(ping.len() as u32).to_be_bytes())
        .await
        .map_err(io::Error::other)?;
    send.write_all(&ping).await.map_err(io::Error::other)?;
    send.finish().map_err(io::Error::other)?;
    let mut header = [0; 4];
    recv.read_exact(&mut header)
        .await
        .map_err(io::Error::other)?;
    let len = u32::from_be_bytes(header) as usize;
    if len > 16 * 1024 {
        return Err(P2pError::Connection("oversized ping response".into()));
    }
    let body = recv.read_to_end(len).await.map_err(|err| match err {
        ReadToEndError::TooLong => {
            P2pError::Connection("ping response exceeds its declared length".into())
        }
        ReadToEndError::Read(e) => P2pError::Io(io::Error::other(e)),
    })?;
    if body.len() != len {
        return Err(P2pError::Connection("incomplete ping response".into()));
    }
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_pool::isolated_pair;
    use crate::node::{EXPLICIT_CATALOG_ALPN, SOUNDTIME_ALPN};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn silent_session_is_replaced_once_with_current_subscription() -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(15), async {
            let (server, pool) = isolated_pair(EXPLICIT_CATALOG_ALPN).await?;
            let checks = AtomicUsize::new(0);
            let request = query(&pool, server.id(), false, true, || async {
                Ok(Some(checks.fetch_add(1, Ordering::Relaxed) == 0))
            });
            let replies = async {
                let stale = server.accept().await.unwrap().await?;
                let (_held, mut first) = stale.accept_bi().await?;
                first.read_to_end(1024).await?;
                // The first connection stays open but never returns a Pong.
                let fresh = server.accept().await.unwrap().await?;
                assert_ne!(fresh.stable_id(), stale.stable_id());
                let (mut send, mut recv) = fresh.accept_bi().await?;
                let frame = recv.read_to_end(1024).await?;
                assert!(matches!(
                    serde_json::from_slice(&frame[4..])?,
                    P2pMessage::Presence {
                        follows_recipient: false,
                        ..
                    }
                ));
                let bytes = serde_json::to_vec(&P2pMessage::PresenceReply {
                    node_id: server.id().to_string(),
                    track_count: 0,
                    version: None,
                    share_identity: false,
                    follows_recipient: false,
                    versioned_catalog: true,
                })?;
                send.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
                send.write_all(&bytes).await?;
                send.finish()?;
                let _ = send.stopped().await?;
                Ok::<_, anyhow::Error>(())
            };
            let (response, served) = tokio::join!(request, replies);
            served?;
            assert!(matches!(response?.1, P2pMessage::PresenceReply { .. }));
            assert_eq!(checks.load(Ordering::Relaxed), 2);
            assert_eq!(pool.established(), 2);
            Ok(())
        })
        .await?
    }

    #[tokio::test]
    async fn failed_presence_rechecks_ban_and_never_repeats_legacy_catalogue_ping(
    ) -> anyhow::Result<()> {
        for alpn in [EXPLICIT_CATALOG_ALPN, SOUNDTIME_ALPN] {
            let (server, pool) = isolated_pair(alpn).await?;
            let checks = AtomicUsize::new(0);
            let request = query(&pool, server.id(), false, false, || async {
                if checks.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(Some(true))
                } else {
                    Err(P2pError::PeerBlocked(server.id().to_string()))
                }
            });
            let failure = async {
                let conn = server.accept().await.unwrap().await?;
                let (mut send, mut recv) = conn.accept_bi().await?;
                recv.read_to_end(1024).await?;
                send.reset(1u32.into())?;
                Ok::<_, anyhow::Error>(())
            };
            let (response, served) = tokio::time::timeout(Duration::from_secs(12), async {
                tokio::join!(request, failure)
            })
            .await?;
            served?;
            assert_eq!(pool.established(), 1);
            if alpn == EXPLICIT_CATALOG_ALPN {
                assert!(matches!(response, Err(P2pError::PeerBlocked(_))));
                assert_eq!(checks.load(Ordering::Relaxed), 2);
            } else {
                assert!(matches!(response, Err(P2pError::Io(_))));
                assert_eq!(checks.load(Ordering::Relaxed), 1);
            }
        }
        Ok(())
    }
}
