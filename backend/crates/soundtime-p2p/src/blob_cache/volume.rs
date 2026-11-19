//! Single-owner cache volume. A directory rename publishes the data and its Bao outboard together.
use crate::range_transport::WindowRequest;
use anyhow::{ensure, Context, Result};
use bao_tree::{
    io::{
        outboard::PreOrderOutboard,
        sync::{encode_ranges_validated, CreateOutboard},
    },
    BaoTree,
};
use bytes::Bytes;
use iroh_blobs::{
    protocol::{ChunkRanges, ChunkRangesExt},
    store::IROH_BLOCK_SIZE,
    Hash,
};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[cfg(unix)]
fn allocated(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks().saturating_mul(512)
}
#[cfg(not(unix))]
fn allocated(meta: &fs::Metadata) -> u64 {
    meta.len()
}

#[cfg(unix)]
fn capacity(path: &Path) -> io::Result<(u64, u64)> {
    let stat = rustix::fs::statvfs(path)?;
    Ok((
        stat.f_bavail.saturating_mul(stat.f_frsize),
        stat.f_frsize.max(4096),
    ))
}
#[cfg(not(unix))]
fn capacity(_: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::other("volume allocation unavailable"))
}

fn footprint(path: &Path) -> io::Result<u64> {
    let meta = fs::symlink_metadata(path)?;
    let mut sum = allocated(&meta);
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            sum = sum.saturating_add(footprint(&entry?.path())?);
        }
    }
    Ok(sum)
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

struct Entry {
    bytes: u64,
    touched: SystemTime,
    readers: u32,
    pinned: bool,
}

#[derive(Default)]
struct State {
    entries: HashMap<Hash, Entry>,
    pending: HashMap<Hash, u64>,
    foreign: u64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct CacheUsage {
    pub maximum_bytes: u64,
    pub reserve_free_bytes: u64,
    pub available_bytes: u64,
    pub evictable_bytes: u64,
    pub pinned_bytes: u64,
    pub reserved_bytes: u64,
    pub legacy_and_unclassified_bytes: u64,
}

pub struct CacheVolume {
    root: PathBuf,
    maximum: u64,
    min_free: u64,
    state: Mutex<State>,
    // The file stays locked until all readers/writers and the node have gone away.
    _lock: File,
    evicted: tokio::sync::broadcast::Sender<Hash>,
}

impl CacheVolume {
    pub fn open(root: PathBuf, maximum: u64, min_free: u64, legacy: u64) -> Result<Arc<Self>> {
        fs::create_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(".owner"))?;
        lock.try_lock()
            .context("cache volume already has an owner")?;
        let mut state = State {
            foreign: legacy,
            ..Default::default()
        };
        // A partial directory can never be an admitted asset. An interrupted
        // eviction remains under the reserved .evict- prefix until its last unlink.
        for child in fs::read_dir(&root)? {
            let child = child?;
            let path = child.path();
            let name = child.file_name();
            let name = name.to_string_lossy();
            if child.file_type()?.is_dir() {
                if name
                    .strip_prefix(".part-")
                    .or_else(|| name.strip_prefix(".evict-"))
                    .is_some_and(|h| h.parse::<Hash>().is_ok())
                {
                    fs::remove_dir_all(&path)?;
                    continue;
                }
                if let Ok(hash) = name.parse::<Hash>() {
                    if hash.to_string() == name
                        && path.join("data").is_file()
                        && path.join("proof").is_file()
                    {
                        state.entries.insert(
                            hash,
                            Entry {
                                bytes: footprint(&path)?,
                                touched: child.metadata()?.modified()?,
                                readers: 0,
                                pinned: path.join("pin").try_exists()?,
                            },
                        );
                        continue;
                    }
                }
            }
            state.foreign = state.foreign.saturating_add(footprint(&path)?);
        }
        sync_dir(&root)?;
        let volume = Arc::new(Self {
            root,
            maximum,
            min_free,
            state: Mutex::new(state),
            _lock: lock,
            evicted: tokio::sync::broadcast::channel(128).0,
        });
        {
            let mut state = volume.state.lock().expect("cache volume");
            volume.make_room(&mut state, 0)?;
        }
        Ok(volume)
    }

    pub fn usage(&self) -> Result<CacheUsage> {
        let s = self.state.lock().expect("cache volume");
        Ok(CacheUsage {
            maximum_bytes: self.maximum,
            reserve_free_bytes: self.min_free,
            available_bytes: capacity(&self.root)?.0,
            evictable_bytes: s
                .entries
                .values()
                .filter(|e| !e.pinned)
                .map(|e| e.bytes)
                .sum(),
            pinned_bytes: s
                .entries
                .values()
                .filter(|e| e.pinned)
                .map(|e| e.bytes)
                .sum(),
            reserved_bytes: s.pending.values().sum(),
            legacy_and_unclassified_bytes: s.foreign,
        })
    }

    fn make_room(&self, state: &mut State, wanted: u64) -> Result<bool> {
        loop {
            let pending = state
                .pending
                .values()
                .copied()
                .fold(wanted, u64::saturating_add);
            let used = state
                .entries
                .values()
                .fold(state.foreign, |n, e| n.saturating_add(e.bytes));
            if used.saturating_add(pending) <= self.maximum
                && capacity(&self.root)?.0 >= self.min_free.saturating_add(pending)
            {
                return Ok(true);
            }
            let Some(hash) = state
                .entries
                .iter()
                .filter(|(_, e)| !e.pinned && e.readers == 0)
                .min_by_key(|(_, e)| e.touched)
                .map(|(h, _)| *h)
            else {
                return Ok(false);
            };
            let path = self.root.join(hash.to_string());
            let evict = self.root.join(format!(".evict-{hash}"));
            fs::rename(&path, &evict)?;
            if let Err(error) = fs::remove_dir_all(&evict) {
                // Keep the full charge until startup reconciliation can finish it.
                let charge = state.entries.remove(&hash).expect("known entry").bytes;
                state.foreign = state.foreign.saturating_add(charge);
                return Err(error.into());
            }
            sync_dir(&self.root)?;
            ensure!(
                !evict.try_exists()? && !path.try_exists()?,
                "cache unlink incomplete"
            );
            state.entries.remove(&hash);
            let _ = self.evicted.send(hash);
        }
    }

    pub fn evictions(&self) -> tokio::sync::broadcast::Receiver<Hash> {
        self.evicted.subscribe()
    }

    pub fn contains(&self, hash: Hash) -> bool {
        self.state
            .lock()
            .expect("cache volume")
            .entries
            .contains_key(&hash)
    }

    pub fn reserve(self: &Arc<Self>, hash: Hash, size: u64) -> Result<Option<CacheWriter>> {
        if self.maximum == 0 {
            return Ok(None);
        }
        let mut state = self.state.lock().expect("cache volume");
        if state.entries.contains_key(&hash) || state.pending.contains_key(&hash) {
            return Ok(None);
        }
        let (_, block) = capacity(&self.root)?;
        let round = |n: u64| n.div_ceil(block).saturating_mul(block);
        let proof = BaoTree::new(size, IROH_BLOCK_SIZE).outboard_size();
        let bytes = round(size)
            .saturating_add(round(proof))
            .saturating_add(4 * block);
        if bytes > self.maximum || !self.make_room(&mut state, bytes)? {
            return Ok(None);
        }
        let dir = self.root.join(format!(".part-{hash}"));
        fs::create_dir(&dir)?;
        let file = match File::create_new(dir.join("data")) {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_dir(&dir);
                return Err(error.into());
            }
        };
        state.pending.insert(hash, bytes);
        Ok(Some(CacheWriter {
            file: Some(file),
            stage: Arc::new(Reservation {
                volume: Arc::clone(self),
                hash,
                dir,
                finalized: AtomicBool::new(false),
            }),
            size,
            written: 0,
        }))
    }

    pub fn pin(&self, hash: Hash, pin: bool) -> Result<bool> {
        let mut state = self.state.lock().expect("cache volume");
        let Some(entry) = state.entries.get_mut(&hash) else {
            return Ok(false);
        };
        let dir = self.root.join(hash.to_string());
        if pin && !entry.pinned {
            File::create_new(dir.join("pin"))?.sync_all()?;
        }
        if !pin && entry.pinned {
            fs::remove_file(dir.join("pin"))?;
        }
        sync_dir(&dir)?;
        entry.pinned = pin;
        entry.bytes = footprint(&dir)?;
        Ok(true)
    }

    pub fn read_guard(self: &Arc<Self>, hash: Hash) -> Option<CacheRead> {
        let mut state = self.state.lock().expect("cache volume");
        let entry = state.entries.get_mut(&hash)?;
        entry.readers += 1;
        entry.touched = SystemTime::now();
        Some(CacheRead {
            volume: Arc::clone(self),
            hash,
        })
    }

    pub async fn read(self: &Arc<Self>, hash: Hash) -> Result<Option<Bytes>> {
        let Some(guard) = self.read_guard(hash) else {
            return Ok(None);
        };
        tokio::task::spawn_blocking(move || {
            let bytes = fs::read(guard.path("data"))?;
            ensure!(Hash::new(&bytes) == hash, "cached content corrupted");
            Ok(Some(bytes.into()))
        })
        .await?
    }

    pub async fn encode(self: &Arc<Self>, req: &WindowRequest) -> Result<Option<Vec<u8>>> {
        let Some(guard) = self.read_guard(req.hash) else {
            return Ok(None);
        };
        let req = req.clone();
        tokio::task::spawn_blocking(move || {
            let file = File::open(guard.path("data"))?;
            ensure!(
                file.metadata()?.len() == req.file_size,
                "cached size differs"
            );
            let tree = BaoTree::new(req.file_size, IROH_BLOCK_SIZE);
            let out = PreOrderOutboard {
                root: req.hash.into(),
                tree,
                data: File::open(guard.path("proof"))?,
            };
            let mut response = req.file_size.to_le_bytes().to_vec();
            encode_ranges_validated(file, &out, &ChunkRanges::bytes(req.range()?), &mut response)?;
            Ok(Some(response))
        })
        .await?
    }
}

pub struct CacheRead {
    volume: Arc<CacheVolume>,
    hash: Hash,
}
impl CacheRead {
    fn path(&self, name: &str) -> PathBuf {
        self.volume.root.join(self.hash.to_string()).join(name)
    }
}
impl Drop for CacheRead {
    fn drop(&mut self) {
        if let Some(e) = self
            .volume
            .state
            .lock()
            .expect("cache volume")
            .entries
            .get_mut(&self.hash)
        {
            e.readers -= 1;
        }
    }
}

struct Reservation {
    volume: Arc<CacheVolume>,
    hash: Hash,
    dir: PathBuf,
    finalized: AtomicBool,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if self.finalized.load(Ordering::Relaxed) {
            return;
        }
        let mut state = self.volume.state.lock().expect("cache volume");
        if let Some(bytes) = state.pending.remove(&self.hash) {
            if fs::remove_dir_all(&self.dir).is_err() && self.dir.exists() {
                state.foreign = state.foreign.saturating_add(bytes);
            }
        }
    }
}

pub struct CacheWriter {
    file: Option<File>,
    stage: Arc<Reservation>,
    size: u64,
    written: u64,
}
impl CacheWriter {
    pub async fn append(&mut self, data: &[u8]) -> Result<()> {
        let next = self
            .written
            .checked_add(data.len() as u64)
            .context("cache size overflow")?;
        ensure!(
            next <= self.size && data.len() <= 1024 * 1024,
            "cache write exceeds reservation or chunk limit"
        );
        let mut file = self.file.take().context("interrupted cache writer")?;
        let guard = Arc::clone(&self.stage);
        let chunk = Bytes::copy_from_slice(data);
        // A cancelled caller leaves the handle and charge in this I/O job until the write finishes.
        self.file = Some(
            tokio::task::spawn_blocking(move || {
                let _reservation = guard;
                file.write_all(&chunk)?;
                Ok::<_, io::Error>(file)
            })
            .await??,
        );
        self.written = next;
        Ok(())
    }

    pub async fn commit(mut self, pin: bool) -> Result<()> {
        ensure!(self.written == self.size, "incomplete cached file");
        let file = self.file.take().context("interrupted cache writer")?;
        let guard = Arc::clone(&self.stage);
        let size = self.size;
        tokio::task::spawn_blocking(move || {
            file.sync_all()?;
            drop(file);
            let data = File::open(guard.dir.join("data"))?;
            let proof = File::create_new(guard.dir.join("proof"))?;
            let mut out = PreOrderOutboard {
                root: guard.hash.into(),
                tree: BaoTree::new(size, IROH_BLOCK_SIZE),
                data: proof,
            };
            out.init_from(data)?;
            ensure!(
                Hash::from(out.root) == guard.hash,
                "cache content hash mismatch"
            );
            out.data.sync_all()?;
            drop(out);
            if pin {
                File::create_new(guard.dir.join("pin"))?.sync_all()?;
            }
            sync_dir(&guard.dir)?;
            let bytes = footprint(&guard.dir)?;
            let mut state = guard.volume.state.lock().expect("cache volume");
            ensure!(
                bytes
                    <= *state
                        .pending
                        .get(&guard.hash)
                        .context("missing file reservation")?,
                "filesystem exceeded reservation"
            );
            fs::rename(&guard.dir, guard.volume.root.join(guard.hash.to_string()))?;
            guard.finalized.store(true, Ordering::Relaxed);
            state.entries.insert(
                guard.hash,
                Entry {
                    bytes,
                    touched: SystemTime::now(),
                    readers: 0,
                    pinned: pin,
                },
            );
            state.pending.remove(&guard.hash);
            drop(state);
            sync_dir(&guard.volume.root)?;
            Ok::<_, anyhow::Error>(())
        })
        .await?
    }
}

pub async fn legacy_charge(store: &iroh_blobs::store::fs::FsStore, root: &Path) -> Result<u64> {
    use futures_util::StreamExt;
    let mut tags = Box::pin(store.tags().list().await?);
    let mut originals = HashSet::new();
    while let Some(tag) = tags.next().await {
        let tag = tag?;
        if tag.name.as_ref() == format!("published-{}", tag.hash).as_bytes() {
            originals.insert(tag.hash);
        }
    }
    // The store's DB can contain inline payloads and unexplained old rows.
    // Charge its allocated pages and unknown external files; keep all existing tags intact.
    let mut total = 0u64;
    for name in ["blobs.db", "temp", "data"] {
        let path = root.join(name);
        if !path.try_exists()? {
            continue;
        }
        if name != "data" {
            total = total.saturating_add(footprint(&path)?);
            continue;
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let protected = entry
                .file_name()
                .to_str()
                .and_then(|n| n.split('.').next())
                .and_then(|h| h.parse::<Hash>().ok())
                .is_some_and(|h| originals.contains(&h));
            if !protected {
                total = total.saturating_add(footprint(&entry.path())?);
            }
        }
    }
    Ok(total)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::sync::Barrier;

    #[tokio::test]
    async fn physical_reservations_protect_pins_and_readers_across_restart() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("cache");
        let (_, block) = capacity(tmp.path())?;
        let size = 512 * 1024u64;
        let limit = size + 8 * block;
        let bytes = vec![39; size as usize];
        let other = vec![40; size as usize];
        let first = Hash::new(&bytes);
        let second = Hash::new(&other);
        let cache = CacheVolume::open(root.clone(), limit, 0, 0)?;
        let gate = Arc::new(Barrier::new(2));
        let reserve = |hash| {
            let (gate, cache) = (Arc::clone(&gate), Arc::clone(&cache));
            tokio::spawn(async move {
                gate.wait().await;
                cache.reserve(hash, size)
            })
        };
        let (a, b) = tokio::join!(reserve(first), reserve(second));
        let (a, b) = (a??, b??);
        assert_ne!(a.is_some(), b.is_some());
        drop((a, b));
        let mut writer = cache.reserve(first, size)?.unwrap();
        writer.append(&bytes).await?;
        writer.commit(true).await?;
        assert!(cache.usage()?.pinned_bytes >= size);
        assert!(cache.reserve(second, size)?.is_none());
        assert!(CacheVolume::open(root.clone(), limit, 0, 0).is_err());
        drop(cache);

        // State left by a killed writer: it has a real allocation but no committed name.
        let staged = root.join(format!(".part-{second}"));
        fs::create_dir(&staged)?;
        fs::write(staged.join("data"), &other)?;
        let cache = CacheVolume::open(root.clone(), limit, 0, 0)?;
        assert!(!staged.exists());
        assert!(cache.contains(first));
        assert!(cache.usage()?.reserved_bytes == 0 && cache.usage()?.pinned_bytes >= size);
        let lease = cache.read_guard(first).unwrap();
        cache.pin(first, false)?;
        assert!(cache.reserve(second, size)?.is_none());
        drop(lease);
        let mut events = cache.evictions();
        let mut writer = cache.reserve(second, size)?.unwrap();
        assert_eq!(events.recv().await?, first);
        assert!(
            !root.join(first.to_string()).exists(),
            "evicted audio still occupies the volume"
        );
        writer.append(&other).await?;
        writer.commit(false).await?;
        assert_eq!(cache.read(second).await?.unwrap().as_ref(), other);
        drop(cache);
        let cache = CacheVolume::open(root.clone(), 0, 0, 0)?;
        assert!(cache.reserve(first, size)?.is_none());
        assert!(!cache.contains(second));
        Ok(())
    }

    #[tokio::test]
    async fn bad_hash_or_unavailable_disk_releases_no_offer() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("cache");
        let hash = Hash::new(b"original");
        let cache = CacheVolume::open(root.clone(), 128 * 1024, 0, 0)?;
        let mut writer = cache.reserve(hash, 8)?.unwrap();
        writer.append(b"replaced").await?;
        assert!(writer.commit(false).await.is_err());
        assert!(!cache.contains(hash));
        assert_eq!(cache.usage()?.reserved_bytes, 0);
        drop(cache);
        let cache = CacheVolume::open(root, 128 * 1024, u64::MAX, 0)?;
        assert!(cache.reserve(hash, 8)?.is_none());
        Ok(())
    }
}
