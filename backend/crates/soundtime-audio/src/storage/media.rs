//! One version of an original for an HTTP request, including requests served from S3.
use super::StorageError;
use aws_sdk_s3::Client;
use std::{fs::Metadata, io, path::Path, time::SystemTime};
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt},
};

#[derive(Clone, PartialEq, Eq)]
struct FileVersion {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(unix)]
    inode: (u64, u64),
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
            #[cfg(unix)]
            inode: (meta.dev(), meta.ino()),
        })
    }
}

/// Opaque identity used to revalidate a verified original before cleanup.
#[derive(PartialEq, Eq)]
pub struct MediaVersion(Version);

#[derive(PartialEq, Eq)]
enum Version {
    Local(FileVersion),
    Object {
        bucket: String,
        key: String,
        etag: Option<String>,
        version: Option<String>,
    },
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

    pub fn version(&self) -> MediaVersion {
        MediaVersion(match &self.source {
            Source::Local(_, stamp) => Version::Local(stamp.clone()),
            Source::Object {
                bucket,
                key,
                etag,
                version,
                ..
            } => Version::Object {
                bucket: bucket.clone(),
                key: key.clone(),
                etag: etag.clone(),
                version: version.clone(),
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

    #[cfg(unix)]
    #[tokio::test]
    async fn atomic_replacement_never_splices_two_originals_into_an_open_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song");
        let replacement = dir.path().join("next");
        let size = 128 * 1024;
        tokio::fs::write(&path, vec![3; size]).await.unwrap();
        tokio::fs::write(&replacement, vec![9; size]).await.unwrap();
        let mut previously_opened = OpenedMedia::local(&path)
            .await
            .unwrap()
            .range(0, size as u64)
            .await
            .unwrap();
        assert_eq!(
            previously_opened.next_chunk().await.unwrap().unwrap(),
            vec![3; 64 * 1024]
        );
        tokio::fs::rename(replacement, &path).await.unwrap();
        // Unix filesystems may update the old inode's ctime when the last directory link disappears.
        match previously_opened.next_chunk().await {
            Ok(Some(old)) => assert_eq!(old, vec![3; 64 * 1024]),
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
            Ok(None) => panic!("truncated old original returned as a complete stream"),
        }
        let mut fresh = OpenedMedia::local(&path)
            .await
            .unwrap()
            .range(0, 4)
            .await
            .unwrap();
        assert_eq!(fresh.next_chunk().await.unwrap().unwrap(), vec![9; 4]);
    }

    #[tokio::test]
    async fn s3_seek_reads_only_the_conditioned_range_and_rejects_a_changed_object() {
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            thread,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let responder = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            for index in 0..4 {
                let mut connection = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(err)
                            if err.kind() == io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("missing S3 request {index}: {err}"),
                    }
                };
                connection.set_nonblocking(false).unwrap();
                connection
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let header: Vec<_> = BufReader::new(&connection)
                    .lines()
                    .map(Result::unwrap)
                    .take_while(|s| !s.is_empty())
                    .collect();
                let header = header.join("\n").to_lowercase();
                let mut request = header.lines().next().unwrap().split_whitespace();
                assert_eq!(
                    request.next(),
                    Some(if index % 2 == 0 { "head" } else { "get" })
                );
                assert_eq!(
                    request.next().unwrap().split('?').next(),
                    Some("/music/test.flac")
                );
                if index % 2 == 0 {
                    connection.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nETag: \"original\"\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    assert!(header.contains("range: bytes=999996-999999"), "{header}");
                    assert!(header.contains("if-match: \"original\""), "{header}");
                    if index == 1 {
                        connection.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 999996-999999/1000000\r\nContent-Length: 4\r\nConnection: close\r\n\r\nTAIL").unwrap();
                    } else {
                        connection.write_all(b"HTTP/1.1 412 Precondition Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    }
                }
            }
        });
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::standard().with_max_attempts(1))
            .timeout_config(
                aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                    .operation_timeout(std::time::Duration::from_secs(5))
                    .build(),
            )
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                "test", "test", None, None, "fixture",
            ))
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(url)
            .force_path_style(true)
            .build();
        let client = Client::from_conf(config);
        let object = OpenedMedia::object(&client, "music", "test.flac".into())
            .await
            .unwrap();
        let mut reader = object.range(999996, 4).await.unwrap();
        assert_eq!(
            reader.next_chunk().await.unwrap().as_deref(),
            Some(b"TAIL".as_slice())
        );
        assert!(reader.next_chunk().await.unwrap().is_none());
        let changed = OpenedMedia::object(&client, "music", "test.flac".into())
            .await
            .unwrap();
        assert!(changed.range(999996, 4).await.is_err());
        responder.join().unwrap();
    }
}
