//! Offline sample-rate conversion (rubato's FFT resampler).

use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};

use crate::{AudioData, AudioError};

/// Converts `data` to `to_hz`. Returns it unchanged if the rate matches.
pub fn resample(data: AudioData, to_hz: u32) -> Result<AudioData, AudioError> {
    let from_hz = data.sample_rate_hz;
    if from_hz == to_hz || data.frames() == 0 {
        return Ok(AudioData {
            sample_rate_hz: to_hz,
            ..data
        });
    }
    let channels = data.channels.len().max(1);
    let frames = data.frames();
    let mut interleaved = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        for ch in &data.channels {
            interleaved.push(ch[i]);
        }
    }
    let input = InterleavedOwned::new_from(interleaved, channels, frames)
        .map_err(|e| AudioError::Resample(e.to_string()))?;
    let mut resampler = Fft::<f32>::new(
        from_hz as usize,
        to_hz as usize,
        1024,
        channels,
        FixedSync::Both,
    )
    .map_err(|e| AudioError::Resample(e.to_string()))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|e| AudioError::Resample(e.to_string()))?;
    let data_out = output.take_data();
    let mut out = vec![Vec::with_capacity(data_out.len() / channels); channels];
    for frame in data_out.chunks_exact(channels) {
        for (c, o) in out.iter_mut().enumerate() {
            o.push(frame[c]);
        }
    }
    Ok(AudioData {
        sample_rate_hz: to_hz,
        channels: out,
    })
}
