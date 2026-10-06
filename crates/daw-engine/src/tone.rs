use std::f64::consts::TAU;

/// Time for the tone to fade in or out. Long enough to avoid clicks, short
/// enough to feel instant.
const FADE_SECONDS: f32 = 0.010;

/// A sine wave with click-free start and stop.
///
/// Real-time safe: no allocation, locking, or I/O after construction.
#[derive(Debug, Clone)]
pub struct ToneGenerator {
    phase: f64,
    phase_step: f64,
    gain: f32,
    target_gain: f32,
    gain_step: f32,
    peak_gain: f32,
}

impl ToneGenerator {
    /// Creates a silent generator. Call [`set_on`](Self::set_on) to start it.
    pub fn new(sample_rate_hz: u32, freq_hz: f64, peak_gain: f32) -> Self {
        let sample_rate_hz = sample_rate_hz.max(1);
        let fade_samples = (FADE_SECONDS * sample_rate_hz as f32).max(1.0);
        let peak_gain = peak_gain.clamp(0.0, 1.0);
        Self {
            phase: 0.0,
            phase_step: freq_hz / f64::from(sample_rate_hz),
            gain: 0.0,
            target_gain: 0.0,
            gain_step: peak_gain / fade_samples,
            peak_gain,
        }
    }

    /// Fades the tone in (`true`) or out (`false`).
    // RT-SAFE
    pub fn set_on(&mut self, on: bool) {
        self.target_gain = if on { self.peak_gain } else { 0.0 };
    }

    /// True while any sound is coming out, including the fade-out tail.
    // RT-SAFE
    pub fn is_sounding(&self) -> bool {
        self.gain > 0.0 || self.target_gain > 0.0
    }

    /// Fills an interleaved buffer, writing the same signal to every channel.
    // RT-SAFE
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_mut(channels.max(1)) {
            let sample = self.next_sample();
            frame.fill(sample);
        }
    }

    // RT-SAFE
    fn next_sample(&mut self) -> f32 {
        if self.gain < self.target_gain {
            self.gain = (self.gain + self.gain_step).min(self.target_gain);
        } else if self.gain > self.target_gain {
            self.gain = (self.gain - self.gain_step).max(self.target_gain);
        }
        if self.gain == 0.0 {
            return 0.0;
        }
        let sample = (self.phase * TAU).sin() as f32 * self.gain;
        // Wrap to keep precision over long playback.
        self.phase = (self.phase + self.phase_step).fract();
        sample
    }
}
