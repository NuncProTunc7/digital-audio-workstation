use daw_model::EffectKind;

use crate::{EffectProcessor, param_id};

/// Volume pulsing: a sine for a smooth throb, or a softened square for a
/// choppy, on-off stutter.
pub struct Tremolo {
    sr: f32,
    rate_hz: f32,
    depth: f32,
    square: bool,
    phase: f32,
}

impl Tremolo {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            rate_hz: 5.0,
            depth: 0.6,
            square: false,
            phase: 0.0,
        }
    }
}

impl EffectProcessor for Tremolo {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Tremolo, index) {
            "rate_hz" => self.rate_hz = value.max(0.0),
            "depth" => self.depth = value.clamp(0.0, 1.0),
            "shape" => self.square = value >= 0.5,
            _ => {}
        }
    }

    fn reset(&mut self) {}

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use std::f32::consts::TAU;
        let inc = self.rate_hz / self.sr;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let wave = (self.phase * TAU).sin();
            // Steep but rounded edges, so the square doesn't click.
            let wave = if self.square {
                (wave * 8.0).tanh()
            } else {
                wave
            };
            let gain = 1.0 - self.depth * (0.5 - 0.5 * wave);
            *l *= gain;
            *r *= gain;
            self.phase = (self.phase + inc).fract();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn the_level_pulses_at_the_rate() {
        let mut fx = with(EffectKind::Tremolo, &[("rate_hz", 4.0), ("depth", 1.0)]);
        let (l, _) = run(fx.as_mut(), &sine(440.0, 0.5, 1.0));
        // 100 ms windows: a 4 Hz pulse goes from nearly silent to full.
        let (lo, hi) = window_levels(&l, 2_400);
        assert!(hi > 0.3 && lo < hi * 0.3, "{lo} .. {hi}");
    }

    #[test]
    fn the_square_shape_is_choppier() {
        let fraction_loud = |shape: f64| {
            let mut fx = with(
                EffectKind::Tremolo,
                &[("rate_hz", 4.0), ("depth", 1.0), ("shape", shape)],
            );
            let (l, _) = run(fx.as_mut(), &vec![0.5; 48_000]);
            l.iter().filter(|s| s.abs() > 0.45).count() as f32 / l.len() as f32
        };
        assert!(fraction_loud(1.0) > fraction_loud(0.0) + 0.15);
    }
}
