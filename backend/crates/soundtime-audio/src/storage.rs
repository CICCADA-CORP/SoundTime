use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("S3 error: {0}")]
    S3(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Trait defining operations all storage backends must implement.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn store_file(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        filename: &str,
        data: &[u8],
    ) -> Result<String, StorageError>;

    fn full_path(&self, relative_path: &str) -> PathBuf;

    async fn file_exists(&self, relative_path: &str) -> bool;

    async fn delete_file(&self, relative_path: &str) -> Result<(), StorageError>;

    async fn store_cover(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        data: &[u8],
    ) -> Result<String, StorageError>;

    async fn read_file(&self, relative_path: &str) -> Result<Vec<u8>, StorageError>;

    async fn hash_file(&self, relative_path: &str) -> Result<String, StorageError> {
        let data = self.read_file(relative_path).await?;
        let hash = Sha256::digest(&data);
        Ok(format!("{:x}", hash))
    }

    async fn list_files(&self, prefix: &str) -> Result<Vec<String>, StorageError>;
}

// ─── Local Filesystem Backend ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AudioStorage {
    base_path: PathBuf,
}

impl AudioStorage {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    pub fn from_env() -> Self {
        let base =
            std::env::var("AUDIO_STORAGE_PATH").unwrap_or_else(|_| "./data/music".to_string());
        Self::new(base)
    }

    pub fn base(&self) -> &Path {
        &self.base_path
    }
}

#[async_trait]
impl StorageBackend for AudioStorage {
    async fn store_file(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        filename: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let album_dir = album_name.unwrap_or("singles");
        let sanitized_album = sanitize_filename(album_dir);
        let sanitized_file = sanitize_filename(filename);

        let dir = self
            .base_path
            .join(user_id.to_string())
            .join(&sanitized_album);

        fs::create_dir_all(&dir).await?;

        let file_path = dir.join(&sanitized_file);

        let final_path = if file_path.exists() {
            let stem = Path::new(&sanitized_file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("audio");
            let ext = Path::new(&sanitized_file)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin");
            let unique = format!("{}_{}.{}", stem, Uuid::new_v4(), ext);
            dir.join(unique)
        } else {
            file_path
        };

        fs::write(&final_path, data).await?;

        let relative = final_path
            .strip_prefix(&self.base_path)
            .unwrap_or(&final_path)
            .to_string_lossy()
            .to_string();

        Ok(relative)
    }

    fn full_path(&self, relative_path: &str) -> PathBuf {
        self.base_path.join(relative_path)
    }

    async fn file_exists(&self, relative_path: &str) -> bool {
        fs::metadata(self.full_path(relative_path)).await.is_ok()
    }

    async fn delete_file(&self, relative_path: &str) -> Result<(), StorageError> {
        let path = self.full_path(relative_path);
        if path.exists() {
            fs::remove_file(path).await?;
        }
        Ok(())
    }

    async fn store_cover(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let album_dir = album_name.unwrap_or("singles");
        let sanitized_album = sanitize_filename(album_dir);

        let dir = self
            .base_path
            .join(user_id.to_string())
            .join(&sanitized_album);

        fs::create_dir_all(&dir).await?;

        let cover_path = dir.join("cover.jpg");
        fs::write(&cover_path, data).await?;

        let relative = cover_path
            .strip_prefix(&self.base_path)
            .unwrap_or(&cover_path)
            .to_string_lossy()
            .to_string();

        Ok(relative)
    }

    async fn read_file(&self, relative_path: &str) -> Result<Vec<u8>, StorageError> {
        let path = self.full_path(relative_path);
        fs::read(&path)
            .await
            .map_err(|_| StorageError::NotFound(relative_path.to_string()))
    }

    async fn list_files(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let dir = self.base_path.join(prefix);
        let mut result = Vec::new();
        if !dir.exists() {
            return Ok(result);
        }
        collect_files_recursive(&dir, &self.base_path, &mut result).await?;
        Ok(result)
    }
}

async fn collect_files_recursive(
    dir: &Path,
    base: &Path,
    result: &mut Vec<String>,
) -> Result<(), StorageError> {
    let mut entries = fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(collect_files_recursive(&path, base, result)).await?;
        } else {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            result.push(rel);
        }
    }
    Ok(())
}

// ─── S3 Backend ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct S3Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
    cache_path: PathBuf,
}

impl S3Storage {
    pub async fn from_config(
        endpoint: Option<&str>,
        region: &str,
        access_key: &str,
        secret_key: &str,
        bucket: &str,
        prefix: &str,
    ) -> Result<Self, StorageError> {
        let creds =
            aws_sdk_s3::config::Credentials::new(access_key, secret_key, None, None, "soundtime");

        let mut config_builder = aws_sdk_s3::Config::builder()
            .region(aws_sdk_s3::config::Region::new(region.to_string()))
            .credentials_provider(creds)
            .behavior_version_latest();

        if let Some(ep) = endpoint {
            config_builder = config_builder.endpoint_url(ep).force_path_style(true);
        }

        let config = config_builder.build();
        let client = aws_sdk_s3::Client::from_conf(config);

        let cache_path = PathBuf::from(
            std::env::var("S3_CACHE_PATH")
                .unwrap_or_else(|_| "/tmp/soundtime-s3-cache".to_string()),
        );
        fs::create_dir_all(&cache_path)
            .await
            .map_err(|e| StorageError::Config(format!("Cannot create S3 cache dir: {e}")))?;

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            cache_path,
        })
    }

    fn s3_key(&self, relative_path: &str) -> String {
        if self.prefix.is_empty() {
            relative_path.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), relative_path)
        }
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn store_file(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        filename: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let album_dir = album_name.unwrap_or("singles");
        let sanitized_album = sanitize_filename(album_dir);
        let sanitized_file = sanitize_filename(filename);

        let relative = format!("{user_id}/{sanitized_album}/{sanitized_file}");
        let key = self.s3_key(&relative);

        let final_relative = match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => {
                let stem = Path::new(&sanitized_file)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("audio");
                let ext = Path::new(&sanitized_file)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("bin");
                format!(
                    "{user_id}/{sanitized_album}/{}_{}.{ext}",
                    stem,
                    Uuid::new_v4()
                )
            }
            Err(_) => relative,
        };

        let final_key = self.s3_key(&final_relative);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&final_key)
            .body(data.to_vec().into())
            .send()
            .await
            .map_err(|e| StorageError::S3(format!("PutObject failed: {e}")))?;

        // Cache locally for streaming
        let cache_file = self.cache_path.join(&final_relative);
        if let Some(parent) = cache_file.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let _ = fs::write(&cache_file, data).await;

        Ok(final_relative)
    }

    fn full_path(&self, relative_path: &str) -> PathBuf {
        self.cache_path.join(relative_path)
    }

    async fn file_exists(&self, relative_path: &str) -> bool {
        let key = self.s3_key(relative_path);
        self.client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .is_ok()
    }

    async fn delete_file(&self, relative_path: &str) -> Result<(), StorageError> {
        let key = self.s3_key(relative_path);
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| StorageError::S3(format!("DeleteObject failed: {e}")))?;

        let cache_file = self.cache_path.join(relative_path);
        let _ = fs::remove_file(cache_file).await;

        Ok(())
    }

    async fn store_cover(
        &self,
        user_id: Uuid,
        album_name: Option<&str>,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let album_dir = album_name.unwrap_or("singles");
        let sanitized_album = sanitize_filename(album_dir);
        let relative = format!("{user_id}/{sanitized_album}/cover.jpg");
        let key = self.s3_key(&relative);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(data.to_vec().into())
            .content_type("image/jpeg")
            .send()
            .await
            .map_err(|e| StorageError::S3(format!("PutObject cover failed: {e}")))?;

        let cache_file = self.cache_path.join(&relative);
        if let Some(parent) = cache_file.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let _ = fs::write(&cache_file, data).await;

        Ok(relative)
    }

    async fn read_file(&self, relative_path: &str) -> Result<Vec<u8>, StorageError> {
        let cache_file = self.cache_path.join(relative_path);
        if cache_file.exists() {
            return fs::read(&cache_file).await.map_err(StorageError::Io);
        }

        let key = self.s3_key(relative_path);
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| StorageError::S3(format!("GetObject failed: {e}")))?;

        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| StorageError::S3(format!("Read body: {e}")))?
            .into_bytes()
            .to_vec();

        if let Some(parent) = cache_file.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let _ = fs::write(&cache_file, &data).await;

        Ok(data)
    }

    async fn list_files(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let s3_prefix = self.s3_key(prefix);
        let mut result = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&s3_prefix);

            if let Some(token) = continuation_token {
                req = req.continuation_token(token);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| StorageError::S3(format!("ListObjects: {e}")))?;

            for obj in resp.contents() {
                if let Some(key) = obj.key() {
                    let rel: String = if self.prefix.is_empty() {
                        key.to_string()
                    } else {
                        key.strip_prefix(&format!("{}/", self.prefix.trim_end_matches('/')))
                            .unwrap_or(key)
                            .to_string()
                    };
                    result.push(rel);
                }
            }
            if resp.is_truncated() == Some(true) {
                continuation_token = resp.next_continuation_token().map(|s| s.to_string());
            } else {
                break;
            }
        }

        Ok(result)
    }
}

// ─── Helpers ───────────────────────────────────────────────────────

pub async fn ensure_local_file(
    storage: &dyn StorageBackend,
    relative_path: &str,
) -> Result<PathBuf, StorageError> {
    let local = storage.full_path(relative_path);
    if local.exists() {
        return Ok(local);
    }
    let data = storage.read_file(relative_path).await?;
    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&local, &data).await?;
    Ok(local)
}

pub fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string();
    // SECURITY: reject path traversal sequences
    if sanitized == ".." || sanitized == "." || sanitized.contains("..") {
        return sanitized.replace("..", "__");
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sanitize_filename_clean() {
        assert_eq!(sanitize_filename("my_song.mp3"), "my_song.mp3");
    }

    #[test]
    fn test_sanitize_filename_slashes() {
        assert_eq!(sanitize_filename("path/to/file"), "path_to_file");
    }

    #[test]
    fn test_sanitize_filename_backslashes() {
        assert_eq!(sanitize_filename("path\\to\\file"), "path_to_file");
    }

    #[test]
    fn test_sanitize_filename_special_chars() {
        assert_eq!(sanitize_filename("a:b*c?d"), "a_b_c_d");
    }

    #[test]
    fn test_sanitize_filename_quotes_and_pipes() {
        assert_eq!(sanitize_filename("a\"b<c>d|e"), "a_b_c_d_e");
    }

    #[test]
    fn test_sanitize_filename_null_byte() {
        assert_eq!(sanitize_filename("a\0b"), "a_b");
    }

    #[test]
    fn test_sanitize_filename_unicode() {
        assert_eq!(sanitize_filename("日本語の曲.mp3"), "日本語の曲.mp3");
    }

    #[test]
    fn test_audio_storage_new() {
        let storage = AudioStorage::new("/tmp/test-storage");
        assert_eq!(storage.base(), Path::new("/tmp/test-storage"));
    }

    #[tokio::test]
    async fn test_store_and_read_file() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_file(user_id, Some("album1"), "song.mp3", b"fake audio data")
            .await
            .unwrap();

        assert!(relative.contains("song.mp3"));

        let data = storage.read_file(&relative).await.unwrap();
        assert_eq!(data, b"fake audio data");
    }

    #[tokio::test]
    async fn test_file_exists() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_file(user_id, Some("album1"), "test.mp3", b"data")
            .await
            .unwrap();

        assert!(storage.file_exists(&relative).await);
        assert!(!storage.file_exists("nonexistent/file.mp3").await);
    }

    #[tokio::test]
    async fn test_delete_file() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_file(user_id, None, "deleteme.mp3", b"data")
            .await
            .unwrap();

        assert!(storage.file_exists(&relative).await);
        storage.delete_file(&relative).await.unwrap();
        assert!(!storage.file_exists(&relative).await);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_file_ok() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let result = storage.delete_file("nonexistent/file.mp3").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_store_cover() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_cover(user_id, Some("album1"), b"fake cover data")
            .await
            .unwrap();

        assert!(relative.contains("cover.jpg"));
        let data = storage.read_file(&relative).await.unwrap();
        assert_eq!(data, b"fake cover data");
    }

    #[tokio::test]
    async fn test_store_file_no_album() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_file(user_id, None, "single.mp3", b"single data")
            .await
            .unwrap();

        assert!(relative.contains("singles"));
    }

    #[tokio::test]
    async fn test_list_files() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        storage
            .store_file(user_id, Some("album"), "a.mp3", b"a")
            .await
            .unwrap();
        storage
            .store_file(user_id, Some("album"), "b.mp3", b"b")
            .await
            .unwrap();

        let files = storage.list_files(&user_id.to_string()).await.unwrap();

        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn test_list_files_empty_prefix() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());

        let files = storage.list_files("nonexistent").await.unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_hash_file() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let relative = storage
            .store_file(user_id, None, "hash_test.mp3", b"hash me")
            .await
            .unwrap();

        let hash = storage.hash_file(&relative).await.unwrap();
        assert!(!hash.is_empty());
        // SHA-256 hash should be 64 hex chars
        assert_eq!(hash.len(), 64);
    }

    #[tokio::test]
    async fn test_hash_file_deterministic() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let rel1 = storage
            .store_file(user_id, Some("a"), "f1.mp3", b"same content")
            .await
            .unwrap();
        let rel2 = storage
            .store_file(user_id, Some("b"), "f2.mp3", b"same content")
            .await
            .unwrap();

        let h1 = storage.hash_file(&rel1).await.unwrap();
        let h2 = storage.hash_file(&rel2).await.unwrap();
        assert_eq!(h1, h2);
    }

    #[tokio::test]
    async fn test_full_path() {
        let storage = AudioStorage::new("/data/music");
        let full = storage.full_path("user1/album/song.mp3");
        assert_eq!(full, PathBuf::from("/data/music/user1/album/song.mp3"));
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let result = storage.read_file("nonexistent.mp3").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_store_file_duplicate_name() {
        let tmp = TempDir::new().unwrap();
        let storage = AudioStorage::new(tmp.path());
        let user_id = Uuid::new_v4();

        let rel1 = storage
            .store_file(user_id, Some("album"), "song.mp3", b"v1")
            .await
            .unwrap();
        let rel2 = storage
            .store_file(user_id, Some("album"), "song.mp3", b"v2")
            .await
            .unwrap();

        // Second file should get a unique name
        assert_ne!(rel1, rel2);
        assert!(storage.file_exists(&rel1).await);
        assert!(storage.file_exists(&rel2).await);
    }
}
