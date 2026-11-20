//! Read only complete RSS items. Large publisher archives need not fit into memory.
use super::{web_url, StatusCode, BAD};
use chrono::DateTime;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use xml::{
    attribute::OwnedAttribute,
    reader::{ParserConfig, XmlEvent},
};

#[derive(Clone, Serialize)]
pub(super) struct Episode {
    id: String,
    title: String,
    description: String,
    published_at: Option<String>,
    audio_url: String,
    duration: Option<u32>,
}

#[derive(Default)]
struct Item {
    guid: String,
    title: String,
    description: String,
    summary: String,
    date: String,
    duration: String,
    audio: Option<String>,
}

impl Item {
    fn text(&mut self, field: &str, content: &str) {
        let target = match field {
            "title" => &mut self.title,
            "description" => &mut self.description,
            "summary" => &mut self.summary,
            "guid" => &mut self.guid,
            "pubDate" => &mut self.date,
            "duration" => &mut self.duration,
            _ => return,
        };
        if target.len() < 8192 {
            target.extend(content.chars().take(8192 - target.len()));
        }
    }

    fn finish(self) -> Option<Episode> {
        let url = self.audio?;
        let title = plain(&self.title, 256);
        if title.is_empty() {
            return None;
        }
        let identity = if self.guid.trim().is_empty() {
            &url
        } else {
            self.guid.trim()
        };
        let id = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let description = plain(
            if self.description.trim().is_empty() {
                &self.summary
            } else {
                &self.description
            },
            1600,
        );
        let date = self.date.trim();
        let published_at = DateTime::parse_from_rfc2822(date)
            .or_else(|_| DateTime::parse_from_rfc3339(date))
            .ok()
            .map(|d| d.to_utc().to_rfc3339());
        Some(Episode {
            id,
            title,
            description,
            published_at,
            audio_url: url,
            duration: duration(&self.duration),
        })
    }
}

fn plain(text: &str, max: usize) -> String {
    // RSS descriptions commonly contain HTML in CDATA. Return text for ordinary UI labels.
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"</?[A-Za-z][^>]*>").unwrap());
    let text = TAG
        .replace_all(text, " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

fn duration(input: &str) -> Option<u32> {
    let mut result: u32 = 0;
    for (i, part) in input.trim().split(':').enumerate() {
        if i > 2 || part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n = part.parse().ok()?;
        if i > 0 && n >= 60 {
            return None;
        }
        result = result.checked_mul(60)?.checked_add(n)?;
    }
    Some(result)
}

fn enclosure(attributes: &[OwnedAttribute]) -> Option<String> {
    let attr = |key| {
        attributes
            .iter()
            .find(|a| a.name.local_name == key)
            .map(|a| a.value.as_str())
    };
    let url = web_url(attr("url")?)?;
    let kind = attr("type")
        .unwrap_or_default()
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    let audio_ext = || {
        let parsed = url::Url::parse(&url).ok()?;
        let ext = parsed.path().rsplit_once('.')?.1.to_ascii_lowercase();
        Some(matches!(
            ext.as_str(),
            "mp3" | "m4a" | "aac" | "ogg" | "opus" | "flac" | "wav"
        ))
    };
    (kind.starts_with("audio/")
        || kind == "application/ogg"
        || ((kind.is_empty() || kind == "application/octet-stream") && audio_ext() == Some(true)))
    .then_some(url)
}

pub(super) fn episodes(bytes: &[u8], truncated: bool) -> Result<(Vec<Episode>, bool), StatusCode> {
    if bytes.windows(9).any(|s| s == b"<!DOCTYPE") {
        return Err(BAD);
    }
    let mut reader = ParserConfig::new()
        .allow_multiple_root_elements(false)
        .max_entity_expansion_depth(0)
        .max_entity_expansion_length(1024)
        .max_attributes(64)
        .max_attribute_length(16384)
        .max_name_length(512)
        .max_data_length(super::MAX_XML)
        .create_reader(std::io::Cursor::new(bytes));
    let mut path = Vec::new();
    let mut current: Option<Item> = None;
    let mut seen = HashSet::new();
    let mut episodes = Vec::new();
    let mut channel = false;
    loop {
        let event = match reader.next() {
            Ok(e) => e,
            Err(_) if truncated && channel && reader.source().position() == bytes.len() as u64 => {
                break
            }
            Err(_) => return Err(BAD),
        };
        match event {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                if path.len() >= 48 {
                    return Err(BAD);
                }
                path.push(name.local_name);
                if path.len() == 1 && path[0] != "rss" {
                    return Err(BAD);
                }
                if path.len() == 2 && path[1] == "channel" {
                    channel = true;
                }
                if path.len() == 3 && path[1] == "channel" && path[2] == "item" {
                    current = Some(Item::default());
                }
                if path.len() == 4 && path[3] == "enclosure" {
                    if let Some(item) = current.as_mut() {
                        if item.audio.is_none() {
                            item.audio = enclosure(&attributes);
                        }
                    }
                }
            }
            XmlEvent::Characters(s) | XmlEvent::CData(s) | XmlEvent::Whitespace(s) => {
                if let (Some(item), Some(field)) = (current.as_mut(), path.get(3)) {
                    item.text(field, &s);
                }
            }
            XmlEvent::EndElement { .. } => {
                if path.len() == 3 {
                    if let Some(item) = current.take().and_then(Item::finish) {
                        if seen.insert(item.id.clone()) {
                            if episodes.len() == 500 {
                                return Ok((episodes, true));
                            }
                            episodes.push(item);
                        }
                    }
                }
                path.pop();
            }
            XmlEvent::EndDocument => break,
            _ => (),
        }
    }
    if !channel {
        return Err(BAD);
    }
    Ok((episodes, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rss_metadata_dates_durations_enclosures_and_stable_ids() {
        let feed = br#"<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
        <item><title>Radio &amp; science</title><guid>42</guid><description><![CDATA[<p>With <i>guests</i></p>]]></description>
        <itunes:duration>1:05:07</itunes:duration><pubDate>Wed, 13 Nov 2024 12:00:00 +0100</pubDate>
        <enclosure type="image/jpeg" url="https://a.example/p.jpg"/>
        <enclosure type="audio/mpeg" url="https://a.example/track?s=1&amp;t=2"/></item>
        <item><title>Duplicate</title><guid>42</guid><enclosure type="audio/mp4" url="https://example.net/other"/></item>
        <item><title>Private IP</title><enclosure type="audio/mpeg" url="http://169.254.169.254/t"/></item>
        <item><title>Video</title><enclosure type="video/mp4" url="https://a.example/x.mp3"/></item>
        <item><title>No MIME</title><enclosure url="https://a.example/x.m4a"/></item>
        </channel></rss>"#;
        let (episodes, truncated) = episodes(feed, false).unwrap();
        assert!(!truncated);
        assert_eq!(episodes.len(), 2);
        let ep = &episodes[0];
        assert_eq!(ep.title, "Radio & science");
        assert_eq!(ep.description, "With guests");
        assert_eq!(ep.audio_url, "https://a.example/track?s=1&t=2");
        assert_eq!(ep.duration, Some(3907));
        assert_eq!(
            ep.published_at.as_deref(),
            Some("2024-11-13T11:00:00+00:00")
        );
        assert_eq!(ep.id.len(), 64);
        assert_eq!(episodes[1].duration, None);
        assert_eq!(duration("4:65"), None);
        assert_eq!(duration("1:2:3:4"), None);
    }
    #[test]
    fn cut_feed_returns_only_whole_items_and_rejects_non_rss_or_dtd() {
        let cut = br#"<rss><channel><item><title>One</title><enclosure type="audio/mpeg" url="https://a.example/one"/></item><item><title>Incomplete"#;
        assert!(episodes(cut, false).is_err());
        let (items, truncated) = episodes(cut, true).unwrap();
        assert!(truncated);
        assert_eq!(items.len(), 1);
        assert!(episodes(b"<html/>", false).is_err());
        assert!(episodes(
            b"<!DOCTYPE rss [<!ENTITY x 'test'>]><rss><channel/></rss>",
            false
        )
        .is_err());
    }
}
