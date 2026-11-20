//! Temporary seekable originals for metadata readers that require a file path.
use super::{OpenedMedia, StorageError};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

pub struct AnalysisFile {
    pub path: PathBuf,
    pub sha256: String,
    // Drop the temporary directory before releasing the workspace lock.
    _scratch: Option<(TempDir, File)>,
}

impl AnalysisFile {
    pub(super) fn local(path: PathBuf, sha256: String) -> Self {
        Self {
            path,
            sha256,
            _scratch: None,
        }
    }

    pub(super) async fn stage(
        object: OpenedMedia,
        cache: &Path,
        name: &str,
    ) -> Result<Self, StorageError> {
        let root = cache.join(".soundtime-analysis-v1");
        tokio::fs::create_dir_all(&root).await?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("lock"))?;
        lock.try_lock().map_err(|_| {
            io::Error::new(io::ErrorKind::WouldBlock, "another S3 analysis is active")
        })?;
        // Only a process holding the volume lock may prepare/remove work directories.
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && entry.file_name().to_string_lossy().starts_with("scan-")
            {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        let size = object.size();
        let max = limit("S3_ANALYSIS_MAX_BYTES", 512 * 1024 * 1024)?;
        if size > max {
            return Err(StorageError::Config(
                "original exceeds S3 analysis budget".into(),
            ));
        }
        let reserve = limit("S3_ANALYSIS_MIN_FREE_BYTES", 256 * 1024 * 1024)?;
        check_free(&root, size.saturating_add(reserve))?;
        let dir = tempfile::Builder::new().prefix("scan-").tempdir_in(&root)?;
        let filename = Path::new(name)
            .file_name()
            .ok_or_else(|| StorageError::Config("missing filename".into()))?;
        let path = dir.path().join(filename);
        let mut file = tokio::fs::File::create_new(&path).await?;
        let mut digest = Sha256::new();
        if size > 0 {
            let mut stream = object.range(0, size).await?;
            let mut since_check = 0;
            while let Some(chunk) = stream.next_chunk().await? {
                if since_check >= 1024 * 1024 {
                    check_free(&root, reserve.saturating_add(1024 * 1024))?;
                    since_check = 0;
                }
                digest.update(&chunk);
                file.write_all(&chunk).await?;
                since_check += chunk.len();
            }
        }
        file.flush().await?;
        Ok(Self {
            path,
            sha256: format!("{:x}", digest.finalize()),
            _scratch: Some((dir, lock)),
        })
    }
}

fn limit(name: &str, default: u64) -> Result<u64, StorageError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map_err(|_| StorageError::Config(format!("{name} must contain a byte count"))),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(_) => Err(StorageError::Config(format!("invalid {name}"))),
    }
}

#[cfg(unix)]
fn check_free(path: &Path, needed: u64) -> io::Result<()> {
    let stats = rustix::fs::statvfs(path)?;
    if stats.f_bavail.saturating_mul(stats.f_frsize) < needed {
        return Err(io::Error::other("insufficient space for S3 analysis"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_free(_: &Path, _: u64) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "S3 analysis space measurement unavailable",
    ))
}
