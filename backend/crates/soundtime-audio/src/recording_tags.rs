//! Identifiers embedded by taggers such as MusicBrainz Picard.
use lofty::{
    config::ParseOptions,
    file::{TaggedFile, TaggedFileExt},
    probe::Probe,
    tag::ItemKey,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordingTags {
    pub recording_id: Option<String>,
    pub isrc: Option<String>,
}

impl RecordingTags {
    pub fn from_file(file: &TaggedFile) -> Self {
        let mut result = Self::default();
        for tag in file.tags() {
            if result.recording_id.is_none() {
                result.recording_id = tag
                    .get_strings(&ItemKey::MusicBrainzRecordingId)
                    .find_map(|s| Uuid::parse_str(s.trim()).ok().filter(|id| !id.is_nil()))
                    .map(|id| id.to_string());
            }
            if result.isrc.is_none() {
                result.isrc = tag.get_strings(&ItemKey::Isrc).find_map(normalize_isrc);
            }
        }
        result
    }

    pub fn read(path: &Path) -> Option<Self> {
        let options = ParseOptions::new()
            .read_properties(false)
            .read_cover_art(false);
        Some(Self::from_file(
            &Probe::open(path).ok()?.options(options).read().ok()?,
        ))
    }
}

fn normalize_isrc(s: &str) -> Option<String> {
    let value = s.trim().replace('-', "").to_ascii_uppercase();
    let bytes = value.as_bytes();
    (bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
        && bytes[5..].iter().all(u8::is_ascii_digit))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_isrc_before_using_it_for_lookup() {
        assert_eq!(
            normalize_isrc("us-abc-23-00123").as_deref(),
            Some("USABC2300123")
        );
        assert!(normalize_isrc("USABC2O00123").is_none());
        assert!(normalize_isrc("not an identifier").is_none());
    }
}
