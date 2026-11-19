//! One version of an original for an HTTP request, including requests served from S3.
use super::StorageError;
use aws_sdk_s3::Client;
use std::{fs::Metadata, io, path::Path, time::SystemTime};
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt},
};

#[derive(PartialEq)]
struct FileVersion {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl FileVersion {
    fn from_metadata(meta: Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected a regular audio file",
            ));
        }
        Ok(Self {
            length: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}

enum Source {
    Local(File, FileVersion),
    Object {
        client: Client,
        bucket: String,
        key: String,
        etag: Option<String>,
        version: Option<String>,
    },
}

pub struct OpenedMedia {
    size: u64,
    source: Source,
}

impl OpenedMedia {
    pub(super) async fn local(path: &Path) -> Result<Self, StorageError> {
        let file = File::open(path).await?;
        let stamp = FileVersion::from_metadata(file.metadata().await?)?;
        Ok(Self {
            size: stamp.length,
            source: Source::Local(file, stamp),
        })
    }

    pub(super) async fn object(
        client: &Client,
        bucket: &str,
        key: String,
    ) -> Result<Self, StorageError> {
        let head = client
            .head_object()
            .bucket(bucket)
            .key(&key)
            .send()
            .await
            .map_err(|err| StorageError::S3(format!("HeadObject: {err}")))?;
        let size = head
            .content_length()
            .and_then(|length| u64::try_from(length).ok())
            .ok_or_else(|| StorageError::S3("invalid object length".into()))?;
        let version = head
            .version_id()
            .filter(|v| !v.is_empty() && *v != "null")
            .map(str::to_owned);
        let etag = head.e_tag().filter(|s| !s.is_empty()).map(str::to_owned);
        if version.is_none() && etag.is_none() {
            return Err(StorageError::S3(
                "object has no stable version or ETag".into(),
            ));
        }
        Ok(Self {
            size,
            source: Source::Object {
                client: client.clone(),
                bucket: bucket.into(),
                key,
                version,
                etag,
            },
        })
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub async fn range(self, offset: u64, length: u64) -> Result<MediaReader, StorageError> {
        let end = offset
            .checked_add(length)
            .filter(|end| length > 0 && *end <= self.size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid original range"))?
            - 1;
        let source = match self.source {
            Source::Local(mut file, stamp) => {
                file.seek(io::SeekFrom::Start(offset)).await?;
                Reader::Local(file, stamp)
            }
            Source::Object {
                client,
                bucket,
                key,
                etag,
                version,
            } => {
                let result = client
                    .get_object()
                    .bucket(bucket)
                    .key(key)
                    .set_if_match(etag)
                    .set_version_id(version)
                    .range(format!("bytes={offset}-{end}"))
                    .send()
                    .await
                    .map_err(|err| StorageError::S3(format!("GetObject range: {err}")))?;
                let expected = format!("bytes {offset}-{end}/{}", self.size);
                if result.content_range() != Some(expected.as_str())
                    || result.content_length() != i64::try_from(length).ok()
                {
                    return Err(StorageError::S3(
                        "object did not return the requested range".into(),
                    ));
                }
                Reader::Object(Box::new(result.body.into_async_read()))
            }
        };
        Ok(MediaReader {
            source,
            remaining: length,
        })
    }
}

enum Reader {
    Local(File, FileVersion),
    Object(Box<dyn AsyncRead + Unpin + Send>),
}

pub struct MediaReader {
    source: Reader,
    remaining: u64,
}

impl MediaReader {
    /// Release one bounded window only after checking the local file again.
    /// A single conditional GET covers the complete selected range on object storage.
    pub async fn next_chunk(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let size = self.remaining.min(64 * 1024) as usize;
        let mut bytes = vec![0; size];
        match &mut self.source {
            Reader::Local(file, stamp) => {
                file.read_exact(&mut bytes).await?;
                if FileVersion::from_metadata(file.metadata().await?)? != *stamp {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "original changed during playback",
                    ));
                }
            }
            Reader::Object(reader) => {
                reader.read_exact(&mut bytes).await?;
            }
        }
        self.remaining -= size as u64;
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    #[tokio::test]
    async fn seek_on_original_and_abort_after_an_in_place_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sound");
        let mut data = vec![7; 256 * 1024];
        data[128 * 1024] = 42;
        tokio::fs::write(&path, &data).await.unwrap();
        let original = OpenedMedia::local(&path).await.unwrap();
        assert_eq!(original.size(), data.len() as u64);
        let mut reader = original.range(128 * 1024, 128 * 1024).await.unwrap();
        assert_eq!(
            reader.next_chunk().await.unwrap().unwrap(),
            &data[128 * 1024..192 * 1024]
        );
        File::options()
            .append(true)
            .open(&path)
            .await
            .unwrap()
            .write_all(b"!")
            .await
            .unwrap();
        assert!(reader.next_chunk().await.is_err());
    }
}
