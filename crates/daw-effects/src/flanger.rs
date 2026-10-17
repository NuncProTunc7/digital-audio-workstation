use daw_model::EffectKind;

use crate::{DelayLine, EffectProcessor, param_id};

/// Shortest and longest delay of the sweep.
const MIN_DELAY_S: f32 = 0.0003;
const MAX_SWEEP_S: f32 = 0.006;

/// A very short delay swept by an LFO and fed back on itself: a comb of
/// notches that rushes up and down (the jet-plane whoosh). The two sides
/// sweep a quarter cycle apart.
pub struct Flanger {
    sr: f32,
    lines: [DelayLine; 2],
    rate_hz: f32,
    depth: f32,
    feedback: f32,
    mix: f32,
    phase: f32,
    last: [f32; 2],
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        let max = ((MIN_DELAY_S + MAX_SWEEP_S) * sr) as usize + 4;
        Self {
            sr,
            lines: [DelayLine::new(max), DelayLine::new(max)],
            rate_hz: 0.25,
            depth: 0.7,
            feedback: 0.5,
            mix: 0.5,
            phase: 0.0,
            last: [0.0; 2],
        }
    }
}

impl EffectProcessor for Flanger {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Flanger, index) {
            "rate_hz" => self.rate_hz = value.max(0.0),
            "depth" => self.depth = value.clamp(0.0, 1.0),
            "feedback" => self.feedback = value.clamp(-0.9, 0.9),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
        self.last = [0.0; 2];
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use std::f32::consts::{FRAC_PI_2, TAU};
        let inc = self.rate_hz / self.sr;
        let min = MIN_DELAY_S * self.sr;
        let sweep = MAX_SWEEP_S * self.sr * self.depth;
        let (fb, mix) = (self.feedback, self.mix);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            for (side, x) in [l, r].into_iter().enumerate() {
                let lfo = 0.5 + 0.5 * (self.phase * TAU + side as f32 * FRAC_PI_2).sin();
                let line = &mut self.lines[side];
                line.push(*x + self.last[side] * fb);
                let wet = line.read(min + sweep * lfo);
                self.last[side] = wet;
                *x = *x * (1.0 - 0.5 * mix) + wet * 0.5 * mix;
            }
            self.phase = (self.phase + inc).fract();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn a_steady_tone_whooshes() {
        let mut fx = with(
            EffectKind::Flanger,
            &[
                ("rate_hz", 2.0),
                ("depth", 1.0),
                ("mix", 1.0),
                ("feedback", 0.7),
            ],
        );
        let (l, _) = run(fx.as_mut(), &sine(1_500.0, 0.3, 1.0));
        let (lo, hi) = window_levels(&l[4_800..], 480);
        assert!(hi > lo * 2.0, "{lo} .. {hi}");
        assert!(peak(&l) < 2.0);
    }
}
