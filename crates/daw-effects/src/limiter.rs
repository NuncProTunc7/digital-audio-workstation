use daw_model::EffectKind;

use crate::{DelayLine, EffectProcessor, db_to_gain, one_pole_coef, param_id};

const LOOKAHEAD_S: f32 = 0.0015;

/// Look-ahead peak limiter: turns the gain down just before a peak arrives,
/// so the output stays under the ceiling without harsh clipping.
pub struct Limiter {
    sr: f32,
    input_gain: f32,
    ceiling: f32,
    release_coef: f32,
    lookahead: usize,
    lines: [DelayLine; 2],
    gain: f32,
    target: f32,
    hold: usize,
    attack_step: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let lookahead = ((LOOKAHEAD_S * sr) as usize).max(1);
        Self {
            sr,
            input_gain: 1.0,
            ceiling: db_to_gain(-1.0),
            release_coef: one_pole_coef(0.1, sr),
            lookahead,
            lines: [DelayLine::new(lookahead + 2), DelayLine::new(lookahead + 2)],
            gain: 1.0,
            target: 1.0,
            hold: 0,
            attack_step: 0.0,
        }
    }
}

impl EffectProcessor for Limiter {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Limiter, index) {
            "input_db" => self.input_gain = db_to_gain(value),
            "ceiling_db" => self.ceiling = db_to_gain(value),
            "release_s" => self.release_coef = one_pole_coef(value, self.sr),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
        self.gain = 1.0;
        self.target = 1.0;
        self.hold = 0;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let la = self.lookahead as f32;
        let [ll, lr] = &mut self.lines;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (il, ir) = (*l * self.input_gain, *r * self.input_gain);
            let peak = il.abs().max(ir.abs());
            let needed = if peak > self.ceiling {
                self.ceiling / peak
            } else {
                1.0
            };
            if needed < self.target {
                // New peak on the way: ramp down to it over the look-ahead.
                self.target = needed;
                self.hold = self.lookahead;
                self.attack_step = (self.gain - needed).max(0.0) / la;
            }
            if self.gain > self.target {
                self.gain = (self.gain - self.attack_step).max(self.target);
            } else if self.hold == 0 {
                self.target = 1.0;
                self.gain = 1.0 + (self.gain - 1.0) * self.release_coef;
            }
            self.hold = self.hold.saturating_sub(1);
            ll.push(il);
            lr.push(ir);
            // Safety net: never exceed the ceiling, even on odd transients.
            *l = (ll.read(la) * self.gain).clamp(-self.ceiling, self.ceiling);
            *r = (lr.read(la) * self.gain).clamp(-self.ceiling, self.ceiling);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn output_never_exceeds_ceiling() {
        let mut fx = with(
            EffectKind::Limiter,
            &[("ceiling_db", -3.0), ("input_db", 12.0)],
        );
        let (l, r) = run(fx.as_mut(), &sine(100.0, 0.9, 1.0));
        let ceiling = 10f32.powf(-3.0 / 20.0);
        assert!(peak(&l) <= ceiling + 1e-6 && peak(&r) <= ceiling + 1e-6);
        // And it is actually loud, not just silenced.
        assert!(peak(&l[24_000..]) > ceiling * 0.9);
    }

    #[test]
    fn transparent_below_ceiling_apart_from_latency() {
        let mut fx = with(EffectKind::Limiter, &[]);
        let input = sine(440.0, 0.3, 0.5);
        let (l, _) = run(fx.as_mut(), &input);
        assert!((peak(&l[4_800..]) - 0.3).abs() < 0.01);
    }
}
