use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WaveformError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Decode error: {0}")]
    Decode(String),
}

/// Generate a waveform summary (peak amplitudes) for visualization.
/// Returns a vector of normalized peak values (0.0..1.0) with `num_points` entries.
pub fn generate_waveform(path: &Path, num_points: usize) -> Result<Vec<f32>, WaveformError> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| WaveformError::Decode(e.to_string()))?;

    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| WaveformError::Decode("no default track".into()))?;

    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| WaveformError::Decode(e.to_string()))?;

    // Collect all samples' peak values
    let mut all_peaks: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let num_samples = decoded.capacity();
        let mut sample_buf = SampleBuffer::<f32>::new(num_samples as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);

        let samples = sample_buf.samples();
        let channels = spec.channels.count();

        // Mix to mono and collect peak per chunk
        for chunk in samples.chunks(channels) {
            let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
            all_peaks.push(mono.abs());
        }
    }

    if all_peaks.is_empty() {
        return Ok(vec![0.0; num_points]);
    }

    // Downsample to num_points
    let chunk_size = (all_peaks.len() / num_points).max(1);
    let mut waveform = Vec::with_capacity(num_points);

    for chunk in all_peaks.chunks(chunk_size) {
        let peak = chunk.iter().cloned().fold(0.0f32, f32::max);
        waveform.push(peak);
    }

    // Normalize to 0.0..1.0
    let max_val = waveform.iter().cloned().fold(0.0f32, f32::max);
    if max_val > 0.0 {
        for v in &mut waveform {
            *v /= max_val;
        }
    }

    // Ensure exactly num_points
    waveform.truncate(num_points);
    while waveform.len() < num_points {
        waveform.push(0.0);
    }

    Ok(waveform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // 1. WaveformError Display for Io variant
    #[test]
    fn test_waveform_error_display_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = WaveformError::Io(io_err);
        let msg = err.to_string();
        assert!(msg.starts_with("IO error:"));
        assert!(msg.contains("file missing"));
    }

    // 2. WaveformError Display for Decode variant
    #[test]
    fn test_waveform_error_display_decode() {
        let err = WaveformError::Decode("unsupported codec".to_string());
        let msg = err.to_string();
        assert!(msg.starts_with("Decode error:"));
        assert!(msg.contains("unsupported codec"));
    }

    // 3. Non-existent file → Err(Io)
    #[test]
    fn test_generate_waveform_nonexistent_file() {
        let result = generate_waveform(
            std::path::Path::new("/tmp/nonexistent_audio_file_xyz.wav"),
            100,
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WaveformError::Io(_)));
    }

    // 4. Text file (not audio) → Err(Decode)
    #[test]
    fn test_generate_waveform_text_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notaudio.wav");
        std::fs::write(&path, "this is not audio data").unwrap();

        let result = generate_waveform(&path, 100);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WaveformError::Decode(_)));
    }

    // 5. Valid WAV file → Ok with correct length and normalized values
    #[test]
    fn test_generate_waveform_valid_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");

        // Create a minimal valid WAV file with PCM audio
        create_test_wav(&path, 44100, 1, 16, 44100); // 1 second of audio

        let result = generate_waveform(&path, 50);
        assert!(result.is_ok());

        let waveform = result.unwrap();
        assert_eq!(waveform.len(), 50);

        // All values should be in range [0.0, 1.0]
        for &val in &waveform {
            assert!(val >= 0.0 && val <= 1.0, "value out of range: {val}");
        }
    }

    // 6. Valid WAV with different num_points
    #[test]
    fn test_generate_waveform_different_num_points() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test2.wav");
        create_test_wav(&path, 44100, 1, 16, 44100);

        for num_points in [10, 100, 200, 500] {
            let result = generate_waveform(&path, num_points);
            assert!(result.is_ok(), "failed for num_points={num_points}");
            assert_eq!(result.unwrap().len(), num_points);
        }
    }

    // 7. Stereo WAV file
    #[test]
    fn test_generate_waveform_stereo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        create_test_wav(&path, 44100, 2, 16, 44100);

        let result = generate_waveform(&path, 50);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 50);
    }

    // 8. Very short audio (less samples than num_points)
    #[test]
    fn test_generate_waveform_short_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.wav");
        // Only 100 samples — shorter than most num_points
        create_test_wav(&path, 44100, 1, 16, 100);

        let result = generate_waveform(&path, 200);
        assert!(result.is_ok());
        let waveform = result.unwrap();
        assert_eq!(waveform.len(), 200);
        // Trailing values should be padded with 0.0
    }

    // ─── Helper: create a minimal WAV file ───────────────────────────
    fn create_test_wav(
        path: &std::path::Path,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        num_samples: u32,
    ) {
        let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
        let block_align = channels * bits_per_sample / 8;
        let data_size = num_samples * channels as u32 * bits_per_sample as u32 / 8;
        let file_size = 36 + data_size;

        let mut f = std::fs::File::create(path).unwrap();

        // RIFF header
        f.write_all(b"RIFF").unwrap();
        f.write_all(&file_size.to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();

        // fmt chunk
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap(); // chunk size
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM format
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&sample_rate.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&block_align.to_le_bytes()).unwrap();
        f.write_all(&bits_per_sample.to_le_bytes()).unwrap();

        // data chunk
        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();

        // Write sine wave samples
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin();
            let sample_i16 = (sample * 16000.0) as i16;
            for _ in 0..channels {
                f.write_all(&sample_i16.to_le_bytes()).unwrap();
            }
        }
    }
}
