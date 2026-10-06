use std::f32::consts::TAU;

/// Basic oscillator shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Waveform {
    /// Maps a parameter value (0, 1, 2, 3) to a waveform.
    pub fn from_index(index: f32) -> Self {
        match index.round() as i32 {
            1 => Waveform::Saw,
            2 => Waveform::Square,
            3 => Waveform::Triangle,
            _ => Waveform::Sine,
        }
    }
}

/// Band-limited oscillator using PolyBLEP to suppress aliasing on saw and
/// square waves. Triangle is generated directly; its harmonics fall off fast
/// enough that aliasing is inaudible at musical pitches.
#[derive(Debug, Clone, Default)]
pub struct Oscillator {
    phase: f32,
}

impl Oscillator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(1.0);
    }

    /// Returns the next sample in -1.0..1.0.
    ///
    /// `phase_inc` is frequency / sample rate. `pulse_width` (0.05–0.95) only
    /// affects the square wave.
    // RT-SAFE
    pub fn next(&mut self, waveform: Waveform, phase_inc: f32, pulse_width: f32) -> f32 {
        let t = self.phase;
        let dt = phase_inc.clamp(0.0, 0.5);
        let out = match waveform {
            Waveform::Sine => (t * TAU).sin(),
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let pw = pulse_width.clamp(0.05, 0.95);
                let naive = if t < pw { 1.0 } else { -1.0 };
                naive + poly_blep(t, dt) - poly_blep((t + 1.0 - pw).fract(), dt)
            }
            Waveform::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Polynomial band-limited step: smooths the discontinuity of a naive
/// waveform over one sample on each side.
// RT-SAFE
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Low-frequency oscillator for vibrato and filter sweeps. Output -1.0..1.0.
#[derive(Debug, Clone, Default)]
pub struct Lfo {
    phase: f32,
}

impl Lfo {
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Advances by `samples` and returns the current sine value.
    // RT-SAFE
    pub fn advance(&mut self, rate_hz: f32, sample_rate_hz: f32, samples: usize) -> f32 {
        self.phase = (self.phase + rate_hz * samples as f32 / sample_rate_hz).fract();
        (self.phase * TAU).sin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn render(waveform: Waveform, freq_hz: f32) -> Vec<f32> {
        let mut osc = Oscillator::new();
        (0..SR as usize)
            .map(|_| osc.next(waveform, freq_hz / SR, 0.5))
            .collect()
    }

    fn upward_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn every_waveform_has_correct_pitch_and_range() {
        for wf in [
            Waveform::Sine,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Triangle,
        ] {
            let x = render(wf, 220.0);
            let crossings = upward_crossings(&x);
            assert!((219..=221).contains(&crossings), "{wf:?}: {crossings}");
            // PolyBLEP may overshoot slightly; anything near ±1 is fine.
            assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.1), "{wf:?}");
        }
    }

    #[test]
    fn waveforms_have_no_dc_offset() {
        for wf in [Waveform::Saw, Waveform::Square, Waveform::Triangle] {
            let x = render(wf, 100.0);
            let mean: f32 = x.iter().sum::<f32>() / x.len() as f32;
            assert!(mean.abs() < 0.01, "{wf:?} mean {mean}");
        }
    }

    #[test]
    fn from_index_maps_all_shapes() {
        assert_eq!(Waveform::from_index(0.0), Waveform::Sine);
        assert_eq!(Waveform::from_index(1.0), Waveform::Saw);
        assert_eq!(Waveform::from_index(2.0), Waveform::Square);
        assert_eq!(Waveform::from_index(3.0), Waveform::Triangle);
    }
}
