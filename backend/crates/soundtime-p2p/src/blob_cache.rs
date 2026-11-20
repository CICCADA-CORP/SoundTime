//! Durable storage for complete, verified remote assets.
mod volume;
pub(crate) use volume::legacy_charge;
pub use volume::{CacheRead, CacheUsage, CacheVolume as BlobCache, CacheWriter};
const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) fn budget_from_env() -> anyhow::Result<(u64, u64)> {
    fn get(name: &str, default: u64) -> anyhow::Result<u64> {
        match std::env::var(name) {
            Ok(v) => parse_size(&v).ok_or_else(|| anyhow::anyhow!("invalid byte limit for {name}")),
            Err(std::env::VarError::NotPresent) => Ok(default),
            Err(e) => Err(e.into()),
        }
    }
    Ok((
        get("P2P_CACHE_MAX_SIZE", DEFAULT_MAX_CACHE_BYTES)?,
        get("P2P_CACHE_MIN_FREE", DEFAULT_MIN_FREE_BYTES)?,
    ))
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();

    if let Ok(bytes) = s.parse::<u64>() {
        return Some(bytes);
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("TB") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("GB") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1024u64 * 1024)
    } else {
        (s.strip_suffix("KB")?, 1024u64)
    };

    num_str.trim().parse::<u64>().ok()?.checked_mul(multiplier)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1073741824"), Some(1073741824));
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn test_parse_size_units() {
        assert_eq!(parse_size("1KB"), Some(1024));
        assert_eq!(parse_size("1MB"), Some(1024 * 1024));
        assert_eq!(parse_size("2GB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("1TB"), Some(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_case_insensitive() {
        assert_eq!(parse_size("2gb"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512Mb"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_whitespace() {
        assert_eq!(parse_size("  2GB  "), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512 MB"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("GB"), None);
        assert_eq!(parse_size("18446744073709551615TB"), None);
    }
}
