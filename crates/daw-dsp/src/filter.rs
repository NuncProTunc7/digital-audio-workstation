use std::f32::consts::PI;

/// Which output of the state-variable filter to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    LowPass,
    BandPass,
    HighPass,
}

impl FilterMode {
    /// Maps a parameter value (0, 1, 2) to a mode.
    pub fn from_index(index: f32) -> Self {
        match index.round() as i32 {
            1 => FilterMode::BandPass,
            2 => FilterMode::HighPass,
            _ => FilterMode::LowPass,
        }
    }
}

/// Zero-delay-feedback state-variable filter (topology-preserving transform).
///
/// Stays stable under fast cutoff modulation and at high resonance, which is
/// why it is the default synth filter. Implemented from the published TPT SVF
/// equations; no third-party code.
#[derive(Debug, Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    pub fn new() -> Self {
        let mut f = Self::default();
        f.set(1_000.0, 0.0, 48_000.0);
        f
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// `resonance` runs 0.0 (none) to 1.0 (near self-oscillation).
    // RT-SAFE
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32, sample_rate_hz: f32) {
        let cutoff_hz = cutoff_hz.clamp(10.0, sample_rate_hz * 0.45);
        let g = (PI * cutoff_hz / sample_rate_hz).tan();
        self.k = 2.0 - 1.95 * resonance.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    // RT-SAFE
    pub fn process(&mut self, input: f32, mode: FilterMode) -> f32 {
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::LowPass => v2,
            FilterMode::BandPass => v1,
            FilterMode::HighPass => input - self.k * v1 - v2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    fn rms_through(freq_hz: f32, cutoff_hz: f32, mode: FilterMode) -> f32 {
        let mut f = Svf::new();
        f.set(cutoff_hz, 0.0, SR);
        let n = SR as usize / 2;
        let out: Vec<f32> = (0..n)
            .map(|i| f.process((TAU * freq_hz * i as f32 / SR).sin(), mode))
            .collect();
        // Skip the first 10 ms of settling.
        let tail = &out[480..];
        (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
    }

    #[test]
    fn lowpass_passes_lows_and_cuts_highs() {
        let low = rms_through(100.0, 1_000.0, FilterMode::LowPass);
        let high = rms_through(10_000.0, 1_000.0, FilterMode::LowPass);
        assert!(low > 0.65, "low {low}");
        assert!(high < 0.02, "high {high}");
    }

    #[test]
    fn highpass_passes_highs_and_cuts_lows() {
        let low = rms_through(50.0, 2_000.0, FilterMode::HighPass);
        let high = rms_through(12_000.0, 2_000.0, FilterMode::HighPass);
        assert!(low < 0.01, "low {low}");
        assert!(high > 0.65, "high {high}");
    }

    #[test]
    fn stays_stable_at_max_resonance_with_extreme_input() {
        let mut f = Svf::new();
        let mut rng = crate::Rng::new(1);
        for i in 0..200_000 {
            // Sweep cutoff wildly every sample.
            let cutoff = if i % 2 == 0 { 20.0 } else { 20_000.0 };
            f.set(cutoff, 1.0, SR);
            let y = f.process(rng.next_bipolar() * 4.0, FilterMode::LowPass);
            assert!(y.is_finite() && y.abs() < 1_000.0, "sample {i}: {y}");
        }
    }
}
