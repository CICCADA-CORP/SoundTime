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
