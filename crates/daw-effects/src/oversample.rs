use daw_dsp::{Biquad, BiquadShape};

/// How many times faster clipping stages run than the song.
pub(crate) const FACTOR: usize = 4;

/// Q of the four sections of an 8th-order Butterworth low-pass.
const BUTTERWORTH_Q: [f32; 4] = [0.5098, 0.6013, 0.9000, 2.5629];

/// Runs a waveshaper at [`FACTOR`] times the sample rate, so the harmonics
/// clipping makes above the song's top frequency are filtered away instead
/// of folding back as harsh false tones (aliasing). One per channel.
#[derive(Debug, Clone)]
pub(crate) struct Oversampler {
    up: [Biquad; 4],
    down: [Biquad; 4],
}

impl Oversampler {
    pub fn new(sample_rate_hz: f32) -> Self {
        let fast = sample_rate_hz * FACTOR as f32;
        // Just under the song's own top frequency.
        let cutoff = sample_rate_hz * 0.45;
        let filters = || {
            BUTTERWORTH_Q.map(|q| {
                let mut b = Biquad::new();
                b.set(BiquadShape::LowPass, cutoff, q, fast);
                b
            })
        };
        Self {
            up: filters(),
            down: filters(),
        }
    }

    pub fn reset(&mut self) {
        self.up
            .iter_mut()
            .chain(self.down.iter_mut())
            .for_each(Biquad::reset);
    }

    /// One song-rate sample through `shape` at the faster rate.
    // RT-SAFE
    #[inline]
    pub fn process(&mut self, x: f32, mut shape: impl FnMut(f32) -> f32) -> f32 {
        let mut out = 0.0;
        for k in 0..FACTOR {
            // Zero-stuffing: one real sample, then zeros, scaled back up.
            let mut u = if k == 0 { x * FACTOR as f32 } else { 0.0 };
            for f in &mut self.up {
                u = f.process(u);
            }
            let mut y = shape(u);
            for f in &mut self.down {
                y = f.process(y);
            }
            out = y;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;

    #[test]
    fn passes_a_clean_signal_through() {
        let mut o = Oversampler::new(SR);
        let input = sine(1_000.0, 0.5, 0.2);
        let out: Vec<f32> = input.iter().map(|&x| o.process(x, |u| u)).collect();
        let tail = &out[2_000..];
        assert!(
            (rms(tail) - rms(&input[2_000..])).abs() < 0.01,
            "{}",
            rms(tail)
        );
    }
}
