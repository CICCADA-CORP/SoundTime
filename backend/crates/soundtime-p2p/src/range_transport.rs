//! Opt-in publication-scoped range protocol. Each bidirectional QUIC stream carries one verified window.

use std::{future::Future, ops::Range, time::Duration};

use anyhow::{anyhow, bail, ensure, Context, Result};
use bao_tree::{io::sync::DecodeResponseIter, io::BaoContentItem, BaoTree};
use bytes::Bytes;
use iroh::{endpoint::Connection, EndpointId};
use iroh_blobs::{
    api::Store,
    protocol::{ChunkRanges, ChunkRangesExt},
    store::IROH_BLOCK_SIZE,
    Hash,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

pub const ALPN: &[u8] = b"soundtime/range/2";
pub const WINDOW_BYTES: u32 = 64 * 1024;
const MAX_HEADER: usize = 512;
// Space for one window, both unaligned 16 KiB leaf groups and proof paths.
const MAX_RESPONSE: usize = 128 * 1024;

/// The caller obtains these values from its authorized publication manifest.
/// A mirror may have a different authenticated identity than the origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRequest {
    pub origin: EndpointId,
    pub publication: Uuid,
    pub revision: u64,
    pub hash: Hash,
    pub file_size: u64,
    pub offset: u64,
    pub length: u32,
}

impl WindowRequest {
    pub(crate) fn range(&self) -> Result<Range<u64>> {
        ensure!(
            (1..=WINDOW_BYTES).contains(&self.length),
            "invalid window length"
        );
        ensure!(self.file_size <= i64::MAX as u64, "unsupported file size");
        let end = self
            .offset
            .checked_add(u64::from(self.length))
            .context("window overflow")?;
        ensure!(end <= self.file_size, "window exceeds file size");
        Ok(self.offset..end)
    }
}

#[derive(Debug)]
pub struct VerifiedWindow {
    pub data: Bytes,
    /// Encoded Bao response including its size and proofs, excluding QUIC framing.
    pub response_bytes: usize,
}

/// Read exactly one window. Dropping this future drops the QUIC streams and
/// stops the response. No bytes are exposed until the entire window verifies.
pub async fn fetch_window(
    connection: &Connection,
    request: &WindowRequest,
    mut local_grant: watch::Receiver<bool>,
) -> Result<VerifiedWindow> {
    ensure!(connection.alpn() == ALPN, "unexpected ALPN");
    request.range()?;
    let header = serde_json::to_vec(request)?;
    ensure!(header.len() <= MAX_HEADER, "range header too long");
    let encoded = tokio::select! {
        biased;
        _ = local_grant.wait_for(|active| !*active) => bail!("local publication grant revoked"),
        result = async {
            let (mut send, mut recv) = connection.open_bi().await?;
            send.write_all(&header).await?;
            send.finish()?;
            Ok::<_, anyhow::Error>(recv.read_to_end(MAX_RESPONSE).await?)
        } => result?,
    };
    let window = decode_window(request, &encoded)?;
    // A ban may have arrived while verifying the Bao leaves.
    ensure!(
        local_grant.has_changed().is_ok() && *local_grant.borrow(),
        "local publication grant revoked"
    );
    Ok(window)
}

pub(crate) fn decode_window(request: &WindowRequest, encoded: &[u8]) -> Result<VerifiedWindow> {
    let wanted = request.range()?;
    let (header, mut proof) = encoded.split_at_checked(8).context("missing Bao size")?;
    let size = u64::from_le_bytes(header.try_into()?);
    ensure!(size == request.file_size, "Bao size differs from manifest");
    let chunks = ChunkRanges::bytes(wanted.clone());
    let tree = BaoTree::new(size, IROH_BLOCK_SIZE);
    let mut data = Vec::with_capacity(request.length as usize);
    for item in DecodeResponseIter::new(request.hash.into(), tree, &mut proof, &chunks) {
        if let BaoContentItem::Leaf(leaf) = item? {
            let start = leaf.offset.max(wanted.start);
            let end = (leaf.offset + leaf.data.len() as u64).min(wanted.end);
            if start < end {
                ensure!(
                    start == wanted.start + data.len() as u64,
                    "noncontiguous proof"
                );
                data.extend_from_slice(
                    &leaf.data[(start - leaf.offset) as usize..(end - leaf.offset) as usize],
                );
            }
        }
    }
    ensure!(proof.is_empty(), "trailing bytes in proof");
    ensure!(data.len() == request.length as usize, "incomplete window");
    Ok(VerifiedWindow {
        data: data.into(),
        response_bytes: encoded.len(),
    })
}

/// Serve one stream on the authenticated connection. The resolver must match
/// the peer and the complete publication/version tuple against current policy.
/// A grant is terminal once its sender publishes false or closes. Reauthorization
/// requires a fresh grant, including on the receiving instance.
pub async fn serve_window(
    connection: &Connection,
    store: &Store,
    authorize: impl FnOnce(EndpointId, &WindowRequest) -> Option<watch::Receiver<bool>>,
) -> Result<()> {
    serve_with(connection, authorize, |request| {
        encode_window(store, request)
    })
    .await
}

pub(crate) async fn encode_window(store: &Store, request: WindowRequest) -> Result<Vec<u8>> {
    let ranges = ChunkRanges::bytes(request.range()?);
    let mut encoded = Vec::new();
    store
        .blobs()
        .export_bao(request.hash, ranges)
        .write(&mut encoded)
        .await?;
    Ok(encoded)
}

pub(crate) async fn read_request(recv: &mut iroh::endpoint::RecvStream) -> Result<WindowRequest> {
    let header =
        tokio::time::timeout(Duration::from_secs(10), recv.read_to_end(MAX_HEADER)).await??;
    let request: WindowRequest = serde_json::from_slice(&header)?;
    request.range()?;
    Ok(request)
}

async fn serve_with<F: Future<Output = Result<Vec<u8>>>>(
    connection: &Connection,
    authorize: impl FnOnce(EndpointId, &WindowRequest) -> Option<watch::Receiver<bool>>,
    encode: impl FnOnce(WindowRequest) -> F,
) -> Result<()> {
    ensure!(connection.alpn() == ALPN, "unexpected ALPN");
    let (mut send, mut recv) = connection.accept_bi().await?;
    let request = read_request(&mut recv).await?;
    let Some(mut grant) = authorize(connection.remote_id(), &request) else {
        send.reset(1u32.into())?;
        bail!("publication denied");
    };
    let final_grant = grant.clone();
    let stopped = send.stopped();
    let result = tokio::select! {
        biased;
        _ = grant.wait_for(|active| !*active) => Err(anyhow!("publication grant revoked")),
        _ = stopped => Err(anyhow!("reader disconnected")),
        result = async {
            let encoded = encode(request).await?;
            ensure!(encoded.len() <= MAX_RESPONSE, "encoded window exceeds size limit");
            ensure!(final_grant.has_changed().is_ok() && *final_grant.borrow(), "publication grant revoked");
            send.write_all(&encoded).await?;
            send.finish()?;
            Ok(())
        } => result,
    };
    if result.is_err() {
        let _ = send.reset(1u32.into());
    }
    result
}

#[cfg(test)]
mod tests;
