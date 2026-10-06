use std::f32::consts::PI;

/// Classic second-order filter shapes, using the well-known "Audio EQ
/// Cookbook" formulas (Robert Bristow-Johnson, public domain).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BiquadShape {
    LowPass,
    HighPass,
    /// Boost or cut everything below the frequency.
    LowShelf {
        gain_db: f32,
    },
    /// Boost or cut everything above the frequency.
    HighShelf {
        gain_db: f32,
    },
    /// Boost or cut a band around the frequency.
    Peak {
        gain_db: f32,
    },
}

/// Transposed direct form II biquad.
#[derive(Debug, Clone, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// A filter that passes sound unchanged.
    pub fn new() -> Self {
        Self {
            b0: 1.0,
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    // RT-SAFE
    pub fn set(&mut self, shape: BiquadShape, freq_hz: f32, q: f32, sample_rate_hz: f32) {
        let freq = freq_hz.clamp(10.0, sample_rate_hz * 0.49);
        let w0 = 2.0 * PI * freq / sample_rate_hz;
        let (sin, cos) = w0.sin_cos();
        let q = q.max(0.05);
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match shape {
            BiquadShape::LowPass => {
                let b1 = 1.0 - cos;
                (b1 / 2.0, b1, b1 / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            BiquadShape::HighPass => {
                let b1 = -(1.0 + cos);
                (
                    -b1 / 2.0,
                    b1,
                    -b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos,
                    1.0 - alpha,
                )
            }
            BiquadShape::Peak { gain_db } => {
                let a = 10f32.powf(gain_db / 40.0);
                (
                    1.0 + alpha * a,
                    -2.0 * cos,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos,
                    1.0 - alpha / a,
                )
            }
            BiquadShape::LowShelf { gain_db } => {
                let a = 10f32.powf(gain_db / 40.0);
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            BiquadShape::HighShelf { gain_db } => {
                let a = 10f32.powf(gain_db / 40.0);
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    // RT-SAFE
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    fn gain_at(shape: BiquadShape, freq: f32, test_hz: f32) -> f32 {
        let mut f = Biquad::new();
        f.set(shape, freq, 0.707, SR);
        let n = SR as usize / 2;
        let out: Vec<f32> = (0..n)
            .map(|i| f.process((TAU * test_hz * i as f32 / SR).sin()))
            .collect();
        let tail = &out[n / 2..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        rms * std::f32::consts::SQRT_2
    }

    #[test]
    fn shelves_and_peaks_hit_their_gain() {
        let db = |g: f32| 20.0 * g.log10();
        assert!(
            (db(gain_at(
                BiquadShape::LowShelf { gain_db: 12.0 },
                300.0,
                40.0
            )) - 12.0)
                .abs()
                < 0.5
        );
        assert!(
            db(gain_at(
                BiquadShape::LowShelf { gain_db: 12.0 },
                300.0,
                8_000.0
            ))
            .abs()
                < 0.5
        );
        assert!(
            (db(gain_at(
                BiquadShape::HighShelf { gain_db: -9.0 },
                2_000.0,
                15_000.0
            )) + 9.0)
                .abs()
                < 0.5
        );
        assert!(
            (db(gain_at(
                BiquadShape::Peak { gain_db: 6.0 },
                1_000.0,
                1_000.0
            )) - 6.0)
                .abs()
                < 0.3
        );
    }

    #[test]
    fn highpass_removes_rumble() {
        assert!(gain_at(BiquadShape::HighPass, 200.0, 20.0) < 0.02);
        assert!(gain_at(BiquadShape::HighPass, 200.0, 5_000.0) > 0.98);
    }

    #[test]
    fn flat_by_default() {
        let mut f = Biquad::new();
        assert_eq!(f.process(0.5), 0.5);
    }
}
