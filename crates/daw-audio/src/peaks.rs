//! Waveform overviews for drawing clips.

use serde::Serialize;

use crate::AudioData;

/// Waveform resolution: one min/max pair per this fraction of a second.
pub const PEAKS_PER_SECOND: u32 = 200;

/// Min/max of the mono mix in fixed-size buckets, plus the overall peak.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Peaks {
    pub per_second: u32,
    /// Interleaved `[min, max, min, max, ...]`, one pair per bucket.
    pub min_max: Vec<f32>,
    /// Loudest sample, 0.0–1.0+ (for "normalize").
    pub peak: f32,
    pub seconds: f64,
}

pub fn peaks(data: &AudioData) -> Peaks {
    // Fractional bucket width keeps exactly PEAKS_PER_SECOND buckets per
    // second at rates like 44.1 kHz.
    let width = f64::from(data.sample_rate_hz.max(1)) / f64::from(PEAKS_PER_SECOND);
    let frames = data.frames();
    let channels = data.channels.len().max(1) as f32;
    let buckets = (frames as f64 / width).ceil() as usize;
    let mut min_max = Vec::with_capacity(2 * buckets);
    for b in 0..buckets {
        let start = (b as f64 * width).round() as usize;
        let end = (((b + 1) as f64 * width).round() as usize).min(frames);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for i in start..end {
            let s = data.channels.iter().map(|c| c[i]).sum::<f32>() / channels;
            lo = lo.min(s);
            hi = hi.max(s);
        }
        min_max.push(lo);
        min_max.push(hi);
    }
    Peaks {
        per_second: PEAKS_PER_SECOND,
        min_max,
        peak: data.peak(),
        seconds: data.seconds(),
    }
}
