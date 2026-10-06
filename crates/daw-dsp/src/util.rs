/// Converts a MIDI note number (69 = A4) to a frequency in hertz.
// RT-SAFE
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Converts decibels to a linear gain factor.
// RT-SAFE
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Small, fast, deterministic noise source (xorshift32).
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        // Zero is a fixed point of xorshift; avoid it.
        Self { state: seed.max(1) }
    }

    /// White noise in -1.0..1.0.
    // RT-SAFE
    pub fn next_bipolar(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole smoother that removes zipper noise from parameter changes.
#[derive(Debug, Clone)]
pub struct Smoother {
    value: f32,
    target: f32,
    coef: f32,
}

impl Smoother {
    /// `time_seconds` is roughly how long a change takes to settle (~63%).
    pub fn new(initial: f32, time_seconds: f32, sample_rate_hz: f32) -> Self {
        let samples = (time_seconds * sample_rate_hz).max(1.0);
        Self {
            value: initial,
            target: initial,
            coef: (-1.0 / samples).exp(),
        }
    }

    // RT-SAFE
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps straight to `value` with no smoothing.
    // RT-SAFE
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    // RT-SAFE
    pub fn next_value(&mut self) -> f32 {
        self.value = self.target + (self.value - self.target) * self.coef;
        self.value
    }

    pub fn target(&self) -> f32 {
        self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_to_hz_matches_reference_pitches() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_hz(60.0) - 261.626).abs() < 1e-2);
        assert!((midi_to_hz(81.0) - 880.0).abs() < 1e-2);
    }

    #[test]
    fn noise_is_bounded_and_deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..10_000 {
            let x = a.next_bipolar();
            assert!((-1.0..=1.0).contains(&x));
            assert_eq!(x, b.next_bipolar());
        }
    }

    #[test]
    fn smoother_converges() {
        let mut s = Smoother::new(0.0, 0.01, 48_000.0);
        s.set_target(1.0);
        for _ in 0..4_800 {
            s.next_value();
        }
        assert!((s.next_value() - 1.0).abs() < 1e-3);
    }
}
