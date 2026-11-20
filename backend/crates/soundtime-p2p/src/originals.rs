//! Merkle proofs for originals that live on an operator's filesystem or object store.
//! Only the requested leaf groups are read from the original when serving a window.
use super::{BlobKind, P2pError, P2pNode};
use crate::range_transport::WindowRequest;
use anyhow::{ensure, Context, Result};
use bao_tree::{
    io::{
        outboard::PreOrderOutboard,
        sync::{encode_ranges_validated, CreateOutboard, ReadAt},
    },
    iter::BaoChunk,
    BaoTree,
};
use bytes::Bytes;
use iroh_blobs::{
    protocol::{ChunkRanges, ChunkRangesExt},
    store::IROH_BLOCK_SIZE,
    Hash,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

struct LeafGroups {
    start: u64,
    bytes: Vec<u8>,
}

impl ReadAt for LeafGroups {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
        let relative = offset
            .checked_sub(self.start)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "offset outside original window",
                )
            })?;
        self.bytes.as_slice().read_at(relative as u64, buffer)
    }
}

fn groups(req: &WindowRequest) -> Result<std::ops::Range<u64>> {
    let wanted = req.range()?;
    let block = IROH_BLOCK_SIZE.bytes() as u64;
    Ok((wanted.start / block * block)..(wanted.end.div_ceil(block) * block).min(req.file_size))
}

/// Byte positions in the pre-order outboard for the same traversal used by the encoder.
fn pair_offsets(req: &WindowRequest) -> Result<Vec<i32>> {
    let wanted = req.range()?;
    let ranges = if (wanted.end - 1) / 1024 == (req.file_size - 1) / 1024 {
        ChunkRanges::bytes(wanted.start..)
    } else {
        ChunkRanges::bytes(wanted)
    };
    let tree = BaoTree::new(req.file_size, IROH_BLOCK_SIZE);
    let mut offsets = Vec::new();
    for item in tree.ranges_pre_order_chunks_iter_ref(&ranges, 0) {
        if let BaoChunk::Parent { node, .. } = item {
            let index = tree
                .pre_order_offset(node)
                .context("invalid original proof node")?;
            offsets.push(i32::try_from(index * 64)?);
        }
    }
    offsets.sort_unstable();
    Ok(offsets)
}

struct ProofPairs {
    offsets: Vec<i32>,
    data: Vec<u8>,
}

impl ReadAt for ProofPairs {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
        let offset = i32::try_from(offset).map_err(io::Error::other)?;
        let pair = self
            .offsets
            .binary_search(&(offset / 64 * 64))
            .map_err(|_| io::Error::other("missing original proof pair"))?;
        let within = (offset % 64) as usize;
        let n = buffer.len().min(64 - within);
        buffer[..n].copy_from_slice(&self.data[pair * 64 + within..pair * 64 + within + n]);
        Ok(n)
    }
}

async fn read_proof(
    db: &impl ConnectionTrait,
    id: Uuid,
    req: &WindowRequest,
) -> Result<Option<(String, ProofPairs)>> {
    let offsets = pair_offsets(req)?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT t.file_path,octet_length(f.outboard) AS proof_size, COALESCE((
             SELECT string_agg(substring(f.outboard FROM slot+1 FOR 64), ''::bytea ORDER BY ord)
             FROM unnest($4::integer[]) WITH ORDINALITY AS x(slot,ord)), ''::bytea) AS pairs
         FROM federation_original_proofs f JOIN tracks t ON t.id=f.track_id
         WHERE f.track_id=$1 AND f.content_hash=$2 AND f.file_size=$3 AND t.content_hash=f.content_hash",
        [id.into(),req.hash.to_string().into(),i64::try_from(req.file_size)?.into(),offsets.clone().into()])).await?;
    let Some(row) = row else { return Ok(None) };
    let length: i32 = row.try_get("", "proof_size")?;
    let data: Vec<u8> = row.try_get("", "pairs")?;
    ensure!(
        u64::try_from(length)? == BaoTree::new(req.file_size, IROH_BLOCK_SIZE).outboard_size()
            && data.len() == offsets.len() * 64,
        "incomplete original proof"
    );
    Ok(Some((
        row.try_get("", "file_path")?,
        ProofPairs { offsets, data },
    )))
}

fn encode(req: &WindowRequest, proof: impl ReadAt, source: LeafGroups) -> Result<Vec<u8>> {
    let tree = BaoTree::new(req.file_size, IROH_BLOCK_SIZE);
    let out = PreOrderOutboard {
        root: req.hash.into(),
        tree,
        data: proof,
    };
    let mut encoded = req.file_size.to_le_bytes().to_vec();
    encode_ranges_validated(
        source,
        &out,
        &ChunkRanges::bytes(req.range()?),
        &mut encoded,
    )?;
    Ok(encoded)
}

#[cfg_attr(test, derive(Clone))]
struct IndexedOriginal {
    size: u64,
    sha256: String,
    hash: Hash,
    proof: Vec<u8>,
}

fn index(reader: impl Read, size: u64) -> Result<IndexedOriginal> {
    struct DigestReader<R>(R, Sha256);
    impl<R: Read> Read for DigestReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let n = self.0.read(buffer)?;
            self.1.update(&buffer[..n]);
            Ok(n)
        }
    }
    ensure!(
        BaoTree::new(size, IROH_BLOCK_SIZE).outboard_size() < 1 << 30,
        "original exceeds PostgreSQL's single proof size limit"
    );
    let mut reader = DigestReader(reader, Sha256::new());
    let out = PreOrderOutboard::<Vec<u8>>::create_sized(&mut reader, size, IROH_BLOCK_SIZE)?;
    // create_sized only consumes the specified length. Detect append-in-place before binding.
    let mut extra = [0u8; 1];
    ensure!(
        reader.read(&mut extra)? == 0,
        "original grew during analysis"
    );
    Ok(IndexedOriginal {
        size,
        sha256: format!("{:x}", reader.1.finalize()),
        hash: out.root.into(),
        proof: out.data,
    })
}

/// The blocking Bao builder consumes an async, version-checked S3 or filesystem
/// stream in 64 KiB chunks. Backfill needs neither a seek nor a scratch copy.
struct OriginalReader {
    media: Option<soundtime_audio::storage::MediaReader>,
    chunk: io::Cursor<Vec<u8>>,
    runtime: tokio::runtime::Handle,
}
impl Read for OriginalReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.chunk.position() == self.chunk.get_ref().len() as u64 {
            let Some(media) = self.media.as_mut() else {
                return Ok(0);
            };
            let Some(next) = self.runtime.block_on(media.next_chunk())? else {
                return Ok(0);
            };
            self.chunk = io::Cursor::new(next);
        }
        Read::read(&mut self.chunk, buf)
    }
}

// Compare the stored fingerprints in the same transaction that installs the proof.
// A mount can return a different recording under the same path and length.
async fn bind_original(
    db: &impl TransactionTrait,
    id: Uuid,
    path: &str,
    original: IndexedOriginal,
) -> Result<()> {
    let tx = db.begin().await?;
    tx.query_one(Statement::from_string(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(1937011565,1)",
    ))
    .await?;
    let row = tx.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT t.file_hash,t.content_hash,t.file_size FROM tracks t
         WHERE t.id=$1 AND t.file_path=$2 AND t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'
         AND NOT EXISTS (SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn')
         AND NOT EXISTS (SELECT FROM federation_missing_sources m WHERE m.track_id=t.id) FOR UPDATE OF t",
        [id.into(),path.into()])).await?.context("publication moved, withdrawn or missing while hashing")?;
    let sha: Option<String> = row.try_get("", "file_hash")?;
    let blob: Option<String> = row.try_get("", "content_hash")?;
    let size: i64 = row.try_get("", "file_size")?;
    let hash = original.hash.to_string();
    if size != i64::try_from(original.size)?
        || sha
            .as_ref()
            .is_some_and(|s| !s.eq_ignore_ascii_case(&original.sha256))
        || blob
            .as_ref()
            .is_some_and(|s| !s.eq_ignore_ascii_case(&hash))
    {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO federation_missing_sources(track_id) VALUES($1) ON CONFLICT DO NOTHING",
            [id.into()],
        ))
        .await?;
        tx.commit().await?;
        anyhow::bail!("the stored original differs from the publication's fingerprints");
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE tracks SET content_hash=$2,file_hash=COALESCE(file_hash,$3) WHERE id=$1
         AND (content_hash IS DISTINCT FROM $2 OR file_hash IS NULL)",
        [
            id.into(),
            hash.as_str().into(),
            original.sha256.as_str().into(),
        ],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO federation_original_proofs(track_id,content_hash,file_size,outboard) VALUES($1,$2,$3,$4)
         ON CONFLICT(track_id) DO UPDATE SET content_hash=excluded.content_hash,file_size=excluded.file_size,outboard=excluded.outboard",
        [id.into(), hash.into(), size.into(), original.proof.into()])).await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM federation_original_index WHERE track_id=$1",
        [id.into()],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

impl P2pNode {
    pub(super) fn start_original_index(self: &Arc<Self>) {
        let node = Arc::clone(self);
        let mut shutdown = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let work = async {
                loop {
                    // Lease one item only. A worker loss or temporarily unavailable object
                    // leaves a durable retry, without rereading indexed libraries on restart.
                    let item = node.db.query_one(Statement::from_string(DbBackend::Postgres,
                        "WITH due AS (
                           SELECT q.track_id,t.file_path FROM federation_original_index q JOIN tracks t ON t.id=q.track_id
                           WHERE q.retry_at<=now() ORDER BY q.retry_at,q.track_id LIMIT 1 FOR UPDATE OF q SKIP LOCKED
                         ) UPDATE federation_original_index q SET retry_at=now()+interval '5 minutes' FROM due
                           WHERE q.track_id=due.track_id RETURNING q.track_id,due.file_path")).await;
                    match item {
                        Ok(Some(row)) => {
                            let result = async {
                                let id: Uuid = row.try_get("", "track_id")?;
                                let path: String = row.try_get("", "file_path")?;
                                node.index_original(id, &path)
                                    .await
                                    .with_context(|| format!("track {id}"))
                            }
                            .await;
                            if let Err(error) = result {
                                tracing::warn!(error=%format!("{error:#}"),"could not index original");
                            }
                            continue;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error,"could not claim original for indexing")
                        }
                    }
                    tokio::select! {
                        _=node.original_index_wake.notified()=>{},
                        _=tokio::time::sleep(Duration::from_secs(30))=>{}
                    }
                }
            };
            tokio::select! {_=work=>{},_=shutdown.changed()=>{}}
        });
    }

    pub fn wake_original_index(&self) {
        self.original_index_wake.notify_one();
    }

    async fn save_original(&self, id: Uuid, path: &str, original: IndexedOriginal) -> Result<Hash> {
        let hash = original.hash;
        bind_original(&self.db, id, path, original).await?;
        self.published_hashes
            .write()
            .await
            .insert(hash.to_string(), BlobKind::Audio);
        Ok(hash)
    }

    pub async fn publish_stored_track(
        &self,
        id: Uuid,
        path: &str,
        bytes: Bytes,
        immutable: Option<PathBuf>,
    ) -> Result<Hash> {
        let data = bytes.clone();
        let original =
            tokio::task::spawn_blocking(move || index(io::Cursor::new(&data), data.len() as u64))
                .await??;
        let hash = self.save_original(id, path, original).await?;
        // The proof follows the track's current path even after an immutable iroh
        // reference loses its old name. Existing raw-blob clients can still use it.
        if let Some(path) = immutable {
            let _ = self.publish_original(bytes, Some(path)).await.inspect_err(|error| {
                tracing::warn!(%id,%error,"immutable blob reference unavailable; using indexed original");
            });
        }
        Ok(hash)
    }

    async fn index_original(&self, id: Uuid, path: &str) -> Result<()> {
        let media = self.original_storage.open_media(path).await?;
        let size = media.size();
        let reader = OriginalReader {
            media: if size == 0 {
                None
            } else {
                Some(media.range(0, size).await?)
            },
            chunk: io::Cursor::new(Vec::new()),
            runtime: tokio::runtime::Handle::current(),
        };
        let original = tokio::task::spawn_blocking(move || index(reader, size)).await??;
        self.save_original(id, path, original).await?;
        Ok(())
    }

    /// A scan has already staged S3 for metadata extraction. Reuse that exact object.
    pub async fn index_analyzed_original(
        &self,
        id: Uuid,
        path: &str,
        input: soundtime_audio::storage::AnalysisFile,
    ) -> Result<()> {
        let original = tokio::task::spawn_blocking(move || -> Result<_> {
            let file = File::open(&input.path)?;
            let size = file.metadata()?.len();
            let original = index(file, size)?;
            ensure!(
                original.sha256 == input.sha256,
                "original changed during analysis"
            );
            Ok(original)
        })
        .await??;
        self.save_original(id, path, original).await?;
        Ok(())
    }

    pub(super) async fn encode_original(
        &self,
        id: Uuid,
        req: &WindowRequest,
    ) -> Result<Option<Vec<u8>>> {
        let Some((path, proof)) = read_proof(&self.db, id, req).await? else {
            return Ok(None);
        };
        let media = self.original_storage.open_media(&path).await?;
        ensure!(media.size() == req.file_size, "original size changed");
        let span = groups(req)?;
        let length = span.end - span.start;
        let mut reader = media.range(span.start, length).await?;
        let mut bytes = Vec::with_capacity(usize::try_from(length)?);
        while let Some(chunk) = reader.next_chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        ensure!(bytes.len() as u64 == length, "incomplete original range");
        Ok(Some(encode(
            req,
            proof,
            LeafGroups {
                start: span.start,
                bytes,
            },
        )?))
    }

    /// The v1 frame requires a complete blob. Access is checked by the caller;
    /// read a single stable original and verify the full hash before returning.
    pub(super) async fn original_blob(&self, hash: Hash) -> Result<Bytes, P2pError> {
        let row = self.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT t.file_path,f.file_size FROM federation_original_proofs f JOIN tracks t ON t.id=f.track_id
             JOIN track_publications p ON p.track_id=t.id AND p.state='active'
             WHERE f.content_hash=$1 AND t.content_hash=f.content_hash
             AND NOT EXISTS(SELECT FROM federation_missing_sources m WHERE m.track_id=t.id) LIMIT 1",
            [hash.to_string().into()])).await?.ok_or_else(|| P2pError::TrackNotFound(hash.to_string()))?;
        let path: String = row.try_get("", "file_path")?;
        let size: i64 = row.try_get("", "file_size")?;
        let verified = async {
            let media = self.original_storage.open_media(&path).await?;
            let size = u64::try_from(size)?;
            ensure!(
                size == media.size() && size <= u64::from(u32::MAX),
                "invalid v1 original size"
            );
            let mut bytes = Vec::new();
            if size > 0 {
                let mut reader = media.range(0, size).await?;
                while let Some(chunk) = reader.next_chunk().await? {
                    bytes.extend_from_slice(&chunk);
                }
            }
            ensure!(
                Hash::new(&bytes) == hash,
                "original no longer matches the requested hash"
            );
            Ok::<_, anyhow::Error>(Bytes::from(bytes))
        }
        .await
        .context("reading a verified legacy original");
        verified.map_err(|e| P2pError::BlobStore(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range_transport::{decode_window, WINDOW_BYTES};
    use bao_tree::io::outboard::PreOrderMemOutboard;

    #[test]
    fn stable_original_can_serve_unaligned_and_tail_windows_without_a_local_copy() -> Result<()> {
        let data: Vec<u8> = (0..(1024 * 1024 + 289)).map(|i| (i % 251) as u8).collect();
        let out = PreOrderMemOutboard::create(&data, IROH_BLOCK_SIZE);
        let origin = iroh::SecretKey::generate().public();
        for (offset, length) in [(3071, WINDOW_BYTES), (data.len() as u64 - 237, 237)] {
            let req = WindowRequest {
                origin,
                publication: Uuid::new_v4(),
                revision: 7,
                hash: Hash::from(out.root),
                file_size: data.len() as u64,
                offset,
                length,
            };
            let span = groups(&req)?;
            assert!(
                span.end - span.start
                    <= u64::from(WINDOW_BYTES) + 2 * IROH_BLOCK_SIZE.bytes() as u64
            );
            let original = data[span.start as usize..span.end as usize].to_vec();
            let offsets = pair_offsets(&req)?;
            let packed = offsets
                .iter()
                .flat_map(|slot| {
                    let i = *slot as usize;
                    out.data[i..i + 64].iter().copied()
                })
                .collect();
            let response = encode(
                &req,
                ProofPairs {
                    offsets,
                    data: packed,
                },
                LeafGroups {
                    start: span.start,
                    bytes: original.clone(),
                },
            )?;
            assert_eq!(
                decode_window(&req, &response)?.data.as_ref(),
                &data[offset as usize..offset as usize + length as usize]
            );
            let mut changed = original;
            changed[(offset - span.start) as usize] ^= 1;
            assert!(encode(
                &req,
                out.data.clone(),
                LeafGroups {
                    start: span.start,
                    bytes: changed
                }
            )
            .is_err());
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires a disposable TEST_DATABASE_URL for PostgreSQL"]
    async fn historical_fingerprints_and_seek_reads_are_bound_to_the_original() -> Result<()> {
        let db = sea_orm::Database::connect(std::env::var("TEST_DATABASE_URL")?).await?;
        let tx = db.begin().await?;
        tx.execute_unprepared("
            CREATE TEMP TABLE tracks(id uuid PRIMARY KEY,file_path text,file_size bigint,file_hash text,content_hash text) ON COMMIT DROP;
            CREATE TEMP TABLE track_publications(track_id uuid,state text) ON COMMIT DROP;
            CREATE TEMP TABLE federation_missing_sources(track_id uuid PRIMARY KEY) ON COMMIT DROP;
            CREATE TEMP TABLE federation_original_index(track_id uuid PRIMARY KEY) ON COMMIT DROP;
            CREATE TEMP TABLE federation_original_proofs(track_id uuid PRIMARY KEY,content_hash text,file_size bigint,outboard bytea) ON COMMIT DROP;
            ALTER TABLE federation_original_proofs ALTER COLUMN outboard SET STORAGE EXTERNAL;").await?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("song");
        let replacement = dir.path().join("replacement");
        std::fs::write(&path, [1; 32768])?;
        std::fs::write(&replacement, [2; 32768])?;
        let original = index(File::open(&path)?, 32768)?;
        std::fs::rename(&replacement, &path)?;
        let changed = index(File::open(&path)?, 32768)?;
        for known_sha in [true, false] {
            let id = Uuid::new_v4();
            let historical_sha = known_sha.then(|| original.sha256.clone());
            let historical_blob = (!known_sha).then(|| original.hash.to_string());
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO tracks VALUES($1,'song',32768,$2,$3)",
                [
                    id.into(),
                    historical_sha.clone().into(),
                    historical_blob.clone().into(),
                ],
            ))
            .await?;
            assert!(bind_original(&tx, id, "song", changed.clone())
                .await
                .is_err());
            let row=tx.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
                "SELECT t.file_hash,t.content_hash,
                   EXISTS(SELECT FROM federation_missing_sources WHERE track_id=t.id) AS missing,
                   EXISTS(SELECT FROM federation_original_proofs WHERE track_id=t.id) AS proven FROM tracks t WHERE t.id=$1",
                [id.into()])).await?.unwrap();
            assert_eq!(
                row.try_get::<Option<String>>("", "file_hash")?,
                historical_sha
            );
            assert_eq!(
                row.try_get::<Option<String>>("", "content_hash")?,
                historical_blob
            );
            assert!(row.try_get::<bool>("", "missing")?);
            assert!(!row.try_get::<bool>("", "proven")?);
        }
        // Exercise the production SQL for a far seek in a 512 MiB original.
        // The complete outboard is about 2 MiB. No audio or full tree is allocated by the reader.
        let id = Uuid::new_v4();
        let size = 512 * 1024 * 1024u64;
        let proof_len = BaoTree::new(size, IROH_BLOCK_SIZE).outboard_size();
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO tracks VALUES($1,'moved/song',$2,NULL,$3)",
            [
                id.into(),
                (size as i64).into(),
                original.hash.to_string().into(),
            ],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO federation_original_proofs VALUES($1,$2,$3,decode(repeat('00',$4),'hex'))",
            [
                id.into(),
                original.hash.to_string().into(),
                (size as i64).into(),
                (proof_len as i32).into(),
            ],
        ))
        .await?;
        let req = WindowRequest {
            origin: iroh::SecretKey::generate().public(),
            publication: id,
            revision: 1,
            hash: original.hash,
            file_size: size,
            offset: size - 2 * u64::from(WINDOW_BYTES) + 311,
            length: WINDOW_BYTES,
        };
        let (path, proof) = read_proof(&tx, id, &req).await?.unwrap();
        assert_eq!(path, "moved/song");
        assert!(!proof.data.is_empty() && proof.data.len() < 2048);
        assert!(proof_len > 1024 * proof.data.len() as u64);
        tx.rollback().await?;
        Ok(())
    }
}
