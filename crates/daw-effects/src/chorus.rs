use daw_model::EffectKind;

use crate::{DelayLine, EffectProcessor, param_id};

const BASE_DELAY_S: f32 = 0.012;
const MAX_SWING_S: f32 = 0.008;

/// Two modulated delay voices per channel, LFOs offset for stereo width.
pub struct Chorus {
    sr: f32,
    lines: [DelayLine; 2],
    phase: f32,
    rate_hz: f32,
    depth: f32,
    mix: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let max = ((BASE_DELAY_S + MAX_SWING_S) * sr) as usize + 4;
        Self {
            sr,
            lines: [DelayLine::new(max), DelayLine::new(max)],
            phase: 0.0,
            rate_hz: 0.8,
            depth: 0.5,
            mix: 0.5,
        }
    }
}

impl EffectProcessor for Chorus {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Chorus, index) {
            "rate_hz" => self.rate_hz = value,
            "depth" => self.depth = value.clamp(0.0, 1.0),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use std::f32::consts::{FRAC_PI_2, TAU};
        let base = BASE_DELAY_S * self.sr;
        let swing = MAX_SWING_S * self.sr * self.depth;
        let inc = self.rate_hz / self.sr;
        let mix = self.mix;
        let [ll, lr] = &mut self.lines;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            ll.push(*l);
            lr.push(*r);
            let p = self.phase * TAU;
            // Two voices per side, a quarter cycle apart between sides.
            let wl =
                0.5 * (ll.read(base + swing * p.sin()) + ll.read(base + swing * (p + 2.1).sin()));
            let wr = 0.5
                * (lr.read(base + swing * (p + FRAC_PI_2).sin())
                    + lr.read(base + swing * (p + FRAC_PI_2 + 2.1).sin()));
            *l = *l * (1.0 - mix * 0.5) + wl * mix;
            *r = *r * (1.0 - mix * 0.5) + wr * mix;
            self.phase = (self.phase + inc).fract();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn widens_a_mono_signal() {
        let mut fx = with(EffectKind::Chorus, &[("depth", 1.0), ("mix", 1.0)]);
        let (l, r) = run(fx.as_mut(), &sine(440.0, 0.4, 1.0));
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        assert!(rms(&diff) > 0.01);
        assert!(peak(&l) < 1.0);
    }
}
