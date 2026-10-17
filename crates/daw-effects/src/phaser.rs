use daw_model::EffectKind;

use crate::{EffectProcessor, param_id};

const STAGES: usize = 4;
/// The sweep's range: notches move between these.
const LOW_HZ: f32 = 200.0;
const HIGH_HZ: f32 = 1_800.0;

/// Four all-pass stages whose turnover frequency an LFO sweeps; mixed with
/// the dry sound they cut moving notches (the classic phaser swirl). The
/// two sides sweep a quarter cycle apart.
pub struct Phaser {
    sr: f32,
    rate_hz: f32,
    depth: f32,
    feedback: f32,
    mix: f32,
    phase: f32,
    /// Per side: each stage's memory, and the last output (for feedback).
    state: [[f32; STAGES]; 2],
    last: [f32; 2],
}

impl Phaser {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            rate_hz: 0.5,
            depth: 0.7,
            feedback: 0.4,
            mix: 0.5,
            phase: 0.0,
            state: [[0.0; STAGES]; 2],
            last: [0.0; 2],
        }
    }
}

impl EffectProcessor for Phaser {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Phaser, index) {
            "rate_hz" => self.rate_hz = value.max(0.0),
            "depth" => self.depth = value.clamp(0.0, 1.0),
            "feedback" => self.feedback = value.clamp(0.0, 0.9),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.state = [[0.0; STAGES]; 2];
        self.last = [0.0; 2];
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let inc = self.rate_hz / self.sr;
        let span = (HIGH_HZ / LOW_HZ).powf(self.depth);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            for (side, x) in [l, r].into_iter().enumerate() {
                let lfo = 0.5 + 0.5 * (self.phase * TAU + side as f32 * FRAC_PI_2).sin();
                // Exponential sweep: equal time per octave.
                let hz = (LOW_HZ * span.powf(lfo)).min(self.sr * 0.45);
                let t = (PI * hz / self.sr).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut v = *x + self.last[side] * self.feedback;
                for z in &mut self.state[side] {
                    let y = a * v + *z;
                    *z = v - a * y;
                    v = y;
                }
                self.last[side] = v;
                *x = *x * (1.0 - 0.5 * self.mix) + v * 0.5 * self.mix;
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
    fn a_steady_tone_swells_and_dips_as_the_notches_move() {
        let mut fx = with(
            EffectKind::Phaser,
            &[
                ("rate_hz", 2.0),
                ("depth", 1.0),
                ("mix", 1.0),
                ("feedback", 0.5),
            ],
        );
        let (l, _) = run(fx.as_mut(), &sine(700.0, 0.4, 1.0));
        let (lo, hi) = window_levels(&l[4_800..], 960);
        assert!(hi > lo * 2.0, "{lo} .. {hi}");
        assert!(peak(&l) < 1.0);
    }
}
