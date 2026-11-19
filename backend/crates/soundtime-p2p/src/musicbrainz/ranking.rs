//! Entity matching uses local evidence. Lucene relevance alone cannot establish identity.
use super::{Hints, Recording, RecordingMatch};
use std::collections::BTreeSet;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub(super) fn normalize(raw: &str) -> String {
    let mut text = String::new();
    for ch in raw
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
    {
        match ch {
            'œ' => text.push_str("oe"),
            'æ' => text.push_str("ae"),
            'ß' => text.push_str("ss"),
            'ø' => text.push('o'),
            'ł' => text.push('l'),
            c if c.is_alphanumeric() => text.push(c),
            _ => text.push(' '),
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn credit_key(raw: &str) -> String {
    let folded = normalize(raw);
    if !folded.contains(' ') {
        return folded;
    }
    folded
        .split_whitespace()
        .filter(|w| !matches!(*w, "feat" | "ft" | "featuring" | "with" | "and" | "x"))
        .collect()
}

fn version(word: &str) -> Option<&'static str> {
    match word {
        "live" | "concert" => Some("live"),
        "remix" | "remixed" | "mix" => Some("mix"),
        "remaster" | "remastered" => Some("remaster"),
        "edit" | "edited" => Some("edit"),
        "acoustic" | "acoustique" | "unplugged" => Some("acoustic"),
        "demo" => Some("demo"),
        "instrumental" | "instrumentale" => Some("instrumental"),
        "karaoke" => Some("karaoke"),
        "extended" => Some("extended"),
        "mono" => Some("mono"),
        "stereo" => Some("stereo"),
        "cover" => Some("cover"),
        "rerecorded" => Some("rerecorded"),
        _ => None,
    }
}

struct Title<'a> {
    base: &'a str,
    core: String,
    kinds: BTreeSet<&'static str>,
    details: BTreeSet<String>,
    guest: String,
}

impl<'a> Title<'a> {
    fn parse(raw: &'a str, comment: &str) -> Self {
        let lower = raw.to_ascii_lowercase();
        let feature = [
            " feat.",
            " ft.",
            " featuring ",
            "(feat.",
            "[feat.",
            "(ft.",
            "[ft.",
        ]
        .iter()
        .filter_map(|s| lower.find(s).map(|at| (at, s.len())))
        .min();
        let (raw, guest) = match feature {
            Some((at, len)) => (
                raw[..at].trim_end_matches([' ', '(', '[']),
                credit_key(&raw[at + len..]),
            ),
            None => (raw, String::new()),
        };
        let mut core = raw;
        let mut descriptor = String::new();
        for (at, ch) in raw.char_indices() {
            let delimiter = matches!(ch, '(' | '[' | '（')
                || (matches!(ch, '-' | '–' | '—') && raw[..at].ends_with(' '));
            if !delimiter {
                continue;
            }
            let tail = normalize(&raw[at..]);
            if tail.split_whitespace().any(|word| version(word).is_some()) {
                core = &raw[..at];
                descriptor = tail;
                break;
            }
        }
        // A bare trailing "remix" or "live" is common in imported filenames.
        if descriptor.is_empty() {
            if let Some((base, suffix)) = raw.rsplit_once(' ') {
                if version(&normalize(suffix)).is_some() {
                    core = base;
                    descriptor = normalize(suffix);
                }
            }
        }
        let comment = normalize(comment);
        if comment
            .split_whitespace()
            .any(|word| version(word).is_some())
        {
            descriptor.push(' ');
            descriptor.push_str(&comment);
        }
        let kinds = descriptor.split_whitespace().filter_map(version).collect();
        let details = descriptor
            .split_whitespace()
            .filter(|s| {
                version(s).is_none()
                    && !matches!(
                        *s,
                        "version"
                            | "recording"
                            | "recorded"
                            | "at"
                            | "in"
                            | "the"
                            | "a"
                            | "on"
                            | "by"
                            | "de"
                            | "le"
                    )
            })
            .map(str::to_owned)
            .collect();
        Self {
            base: core.trim(),
            core: normalize(core).replace(' ', ""),
            kinds,
            details,
            guest,
        }
    }
}

pub(super) fn base_title(raw: &str) -> &str {
    Title::parse(raw, "").base
}

fn one_typo(a: &str, b: &str) -> bool {
    let a: Vec<_> = a.chars().collect();
    let b: Vec<_> = b.chars().collect();
    if a.len().min(b.len()) < 8 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let first = short.iter().zip(long).position(|(a, b)| a != b);
    match first {
        None => true,
        Some(i) if short.len() == long.len() => short[i + 1..] == long[i + 1..],
        Some(i) => short[i..] == long[i + 1..],
    }
}

fn artist_strength(artist: &str, r: &Recording) -> Option<i32> {
    let ask = credit_key(artist);
    let canonical: String = r
        .artist_credit
        .iter()
        .map(|c| format!("{}{}", c.artist.name, c.joinphrase))
        .collect();
    if !ask.is_empty() && (ask == credit_key(&r.credited_artist()) || ask == credit_key(&canonical))
    {
        return Some(32);
    }
    let primary = r.artist_credit.first()?;
    if [
        primary.name.as_deref(),
        Some(primary.artist.name.as_str()),
        primary.artist.sort_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|s| ask == credit_key(s))
    {
        return Some(if r.artist_credit.len() == 1 { 32 } else { 19 });
    }
    None
}

fn evidence(input: &Hints<'_>, title: &Title<'_>, r: &Recording) -> Option<(i32, bool)> {
    let candidate = Title::parse(&r.title, r.disambiguation.as_deref().unwrap_or(""));
    let exact = title.core == candidate.core;
    if title.core.is_empty()
        || (!exact && (input.title_only() || !one_typo(&title.core, &candidate.core)))
        || title.kinds != candidate.kinds
    {
        return None;
    }
    let full_version = title.details.is_subset(&candidate.details);
    if !full_version && !candidate.details.is_empty() {
        return None;
    }
    if !title.guest.is_empty() && title.guest != candidate.guest {
        let guests: String = r
            .artist_credit
            .iter()
            .skip(1)
            .map(|c| credit_key(c.name.as_deref().unwrap_or(&c.artist.name)))
            .collect();
        if guests != title.guest {
            return None;
        }
    }
    let artist = match input.artist {
        Some(s) => artist_strength(s, r)?,
        None => 0,
    };
    let album = input.album.is_some_and(|a| {
        let a = normalize(a);
        r.releases.iter().any(|rel| normalize(&rel.title) == a)
    });
    let difference = input
        .duration_ms
        .zip(r.length.filter(|n| *n > 0))
        .map(|(a, b)| a.abs_diff(b));
    let duration_score = match difference {
        Some(0..=2500) => 16,
        Some(2501..=7000) => 7,
        Some(ms) if ms > 12_000_u64.max(input.duration_ms.unwrap_or(0) / 12) => -30,
        Some(_) => -5,
        None => 0,
    };
    let supported = match artist {
        32 => exact || album || duration_score >= 7,
        19 => exact && (album || duration_score >= 7 || !title.guest.is_empty()),
        _ => exact && (album || (duration_score == 16 && r.score >= 90)),
    };
    let points = (if exact { 50 } else { 36 })
        + artist
        + (if album { 18 } else { 0 })
        + duration_score
        + i32::from(r.score >= 95);
    Some((
        points,
        supported && full_version && duration_score >= 0 && r.score >= 70,
    ))
}

pub(super) fn choose(hints: &Hints<'_>, recordings: &[Recording]) -> RecordingMatch {
    let title = Title::parse(hints.title, "");
    let mut ranked: Vec<_> = recordings
        .iter()
        .filter_map(|r| {
            let (score, reliable) = evidence(hints, &title, r)?;
            Some((score, reliable, r))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.id.cmp(&b.2.id)));
    let Some((best, reliable, recording)) = ranked.first() else {
        return RecordingMatch::Absent;
    };
    let margin = if hints.artist.is_none() && hints.album.is_none() {
        14
    } else {
        8
    };
    if *reliable && ranked.get(1).is_none_or(|next| best - next.0 >= margin) {
        return RecordingMatch::Reliable((*recording).clone());
    }
    RecordingMatch::Ambiguous(
        ranked
            .into_iter()
            .take(5)
            .map(|(_, _, r)| r.clone())
            .collect(),
    )
}
