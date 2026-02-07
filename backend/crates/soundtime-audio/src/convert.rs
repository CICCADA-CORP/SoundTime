use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::process::Command;
use tracing;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("ffmpeg not found — install ffmpeg for AIFF→FLAC conversion")]
    FfmpegNotFound,
    #[error("conversion failed: {0}")]
    ConversionFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Check whether the given extension is an AIFF format that needs conversion.
pub fn needs_aiff_conversion(extension: &str) -> bool {
    matches!(extension.to_lowercase().as_str(), "aif" | "aiff")
}

/// Convert an AIFF file to FLAC using ffmpeg, preserving all metadata.
///
/// Returns the path to the newly created FLAC file. The original AIFF file
/// is removed after successful conversion.
pub async fn convert_aiff_to_flac(input: &Path) -> Result<PathBuf, ConvertError> {
    let output = input.with_extension("flac");

    tracing::info!(
        input = %input.display(),
        output = %output.display(),
        "converting AIFF → FLAC"
    );

    let result = Command::new("ffmpeg")
        .args([
            "-i",
            input.to_str().unwrap_or_default(),
            // Copy all metadata (tags, cover art)
            "-map_metadata", "0",
            // FLAC codec with high compression
            "-c:a", "flac",
            "-compression_level", "8",
            // Also copy attached images (cover art)
            "-c:v", "copy",
            // Overwrite without asking
            "-y",
            output.to_str().unwrap_or_default(),
        ])
        .output()
        .await;

    match result {
        Ok(output_result) => {
            if !output_result.status.success() {
                let stderr = String::from_utf8_lossy(&output_result.stderr);
                tracing::error!(%stderr, "ffmpeg conversion failed");
                return Err(ConvertError::ConversionFailed(stderr.to_string()));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConvertError::FfmpegNotFound);
        }
        Err(e) => {
            return Err(ConvertError::Io(e));
        }
    }

    // Remove original AIFF file
    if let Err(e) = tokio::fs::remove_file(input).await {
        tracing::warn!(error = %e, "failed to remove original AIFF file after conversion");
    }

    tracing::info!(output = %output.display(), "AIFF → FLAC conversion complete");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_aiff_conversion_aif() {
        assert!(needs_aiff_conversion("aif"));
    }

    #[test]
    fn test_needs_aiff_conversion_aiff() {
        assert!(needs_aiff_conversion("aiff"));
    }

    #[test]
    fn test_needs_aiff_conversion_case_insensitive() {
        assert!(needs_aiff_conversion("AIF"));
        assert!(needs_aiff_conversion("AIFF"));
        assert!(needs_aiff_conversion("Aiff"));
    }

    #[test]
    fn test_needs_aiff_conversion_mp3_false() {
        assert!(!needs_aiff_conversion("mp3"));
    }

    #[test]
    fn test_needs_aiff_conversion_flac_false() {
        assert!(!needs_aiff_conversion("flac"));
    }

    #[test]
    fn test_needs_aiff_conversion_wav_false() {
        assert!(!needs_aiff_conversion("wav"));
    }

    #[test]
    fn test_needs_aiff_conversion_empty_false() {
        assert!(!needs_aiff_conversion(""));
    }

    #[test]
    fn test_needs_aiff_conversion_ogg_false() {
        assert!(!needs_aiff_conversion("ogg"));
    }
}
