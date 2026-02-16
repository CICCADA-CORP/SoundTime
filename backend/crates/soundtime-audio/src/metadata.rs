use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Lofty error: {0}")]
    Lofty(#[from] lofty::error::LoftyError),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Album artist (TPE2 / `AlbumArtist` tag). Used to group tracks into albums
    /// independently of the per-track artist. Compilations typically set this to
    /// "Various Artists" so all tracks land under a single album entry.
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_secs: f64,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub format: String,
    pub file_size: u64,
    pub cover_art: Option<Vec<u8>>,
}

/// Supported audio formats
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "aac", "opus", "m4a", "aif", "aiff",
];

/// Check if a file extension is supported
pub fn is_supported_format(extension: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&extension.to_lowercase().as_str())
}

/// Normalize a genre string: split compound genres on common separators,
/// take the first segment, trim whitespace, and title-case it.
pub fn normalize_genre(raw: &str) -> String {
    let first = raw
        .split(&['/', ';', ',', ':', '\\', '|'][..])
        .next()
        .unwrap_or(raw)
        .trim();

    if first.is_empty() {
        return String::new();
    }

    // Title case: capitalize first letter of each word
    first
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract metadata from an audio file using lofty
pub fn extract_metadata_from_file(path: &Path) -> Result<AudioMetadata, MetadataError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if !is_supported_format(&extension) {
        return Err(MetadataError::UnsupportedFormat(extension));
    }

    let file_size = std::fs::metadata(path)?.len();
    let tagged_file = Probe::open(path)?.read()?;

    let properties = tagged_file.properties();
    let duration = properties.duration();
    let duration_secs = duration.as_secs_f64();
    let bitrate = properties.audio_bitrate();
    let sample_rate = properties.sample_rate();
    let channels = properties.channels();

    // Extract tags (try primary tag first, then others)
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let (title, artist, album, album_artist, genre, year, track_number, disc_number, cover_art) =
        if let Some(tag) = tag {
            let cover = tag.pictures().first().map(|p| p.data().to_vec());

            (
                tag.title().map(|t| t.to_string()),
                tag.artist().map(|a| a.to_string()),
                tag.album().map(|a| a.to_string()),
                tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string()),
                tag.genre()
                    .map(|g| normalize_genre(g.as_ref()))
                    .filter(|g| !g.is_empty()),
                tag.year(),
                tag.track(),
                tag.disk(),
                cover,
            )
        } else {
            (None, None, None, None, None, None, None, None, None)
        };

    let format = match extension.as_str() {
        "mp3" => "mp3",
        "flac" => "flac",
        "ogg" | "opus" => "ogg",
        "wav" => "wav",
        "aac" | "m4a" => "aac",
        "aif" | "aiff" => "aiff",
        other => other,
    }
    .to_string();

    Ok(AudioMetadata {
        title,
        artist,
        album,
        album_artist,
        genre,
        year,
        track_number,
        disc_number,
        duration_secs,
        bitrate,
        sample_rate,
        channels,
        format,
        file_size,
        cover_art,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_format_mp3() {
        assert!(is_supported_format("mp3"));
    }

    #[test]
    fn test_is_supported_format_flac() {
        assert!(is_supported_format("flac"));
    }

    #[test]
    fn test_is_supported_format_ogg() {
        assert!(is_supported_format("ogg"));
    }

    #[test]
    fn test_is_supported_format_wav() {
        assert!(is_supported_format("wav"));
    }

    #[test]
    fn test_is_supported_format_aac() {
        assert!(is_supported_format("aac"));
    }

    #[test]
    fn test_is_supported_format_opus() {
        assert!(is_supported_format("opus"));
    }

    #[test]
    fn test_is_supported_format_m4a() {
        assert!(is_supported_format("m4a"));
    }

    #[test]
    fn test_is_supported_format_aiff() {
        assert!(is_supported_format("aif"));
        assert!(is_supported_format("aiff"));
    }

    #[test]
    fn test_is_supported_format_case_insensitive() {
        assert!(is_supported_format("MP3"));
        assert!(is_supported_format("FLAC"));
        assert!(is_supported_format("Wav"));
    }

    #[test]
    fn test_unsupported_formats() {
        assert!(!is_supported_format("txt"));
        assert!(!is_supported_format("pdf"));
        assert!(!is_supported_format("exe"));
        assert!(!is_supported_format(""));
    }

    #[test]
    fn test_extract_metadata_unsupported_format() {
        let tmp = tempfile::NamedTempFile::with_suffix(".txt").unwrap();
        let result = extract_metadata_from_file(tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported format"));
    }

    #[test]
    fn test_extract_metadata_nonexistent_file() {
        let path = Path::new("/tmp/nonexistent_audio_file_12345.mp3");
        let result = extract_metadata_from_file(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_metadata_serialization() {
        let meta = AudioMetadata {
            title: Some("Test Song".into()),
            artist: Some("Test Artist".into()),
            album: Some("Test Album".into()),
            album_artist: Some("Test Album Artist".into()),
            genre: Some("Rock".into()),
            year: Some(2024),
            track_number: Some(1),
            disc_number: Some(1),
            duration_secs: 180.5,
            bitrate: Some(320),
            sample_rate: Some(44100),
            channels: Some(2),
            format: "mp3".into(),
            file_size: 5_000_000,
            cover_art: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"title\":\"Test Song\""));
        assert!(json.contains("\"format\":\"mp3\""));
    }

    #[test]
    fn test_audio_metadata_deserialization() {
        let json = r#"{
            "title": "My Track",
            "artist": null,
            "album": null,
            "album_artist": null,
            "genre": null,
            "year": null,
            "track_number": null,
            "disc_number": null,
            "duration_secs": 60.0,
            "bitrate": null,
            "sample_rate": null,
            "channels": null,
            "format": "flac",
            "file_size": 1000000,
            "cover_art": null
        }"#;
        let meta: AudioMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.title.as_deref(), Some("My Track"));
        assert_eq!(meta.album_artist, None);
        assert_eq!(meta.format, "flac");
        assert_eq!(meta.file_size, 1_000_000);
    }

    #[test]
    fn test_supported_extensions_list() {
        // Ensure we have all 9 supported extensions
        assert_eq!(SUPPORTED_EXTENSIONS.len(), 9);
    }
}
