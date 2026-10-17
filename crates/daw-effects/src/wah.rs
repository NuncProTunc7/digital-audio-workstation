use daw_dsp::{FilterMode, Svf};
use daw_model::EffectKind;

use crate::{EffectProcessor, one_pole_coef, param_id};

/// The pedal's range: heel down to toe down.
const HEEL_HZ: f32 = 350.0;
const TOE_HZ: f32 = 2_200.0;

/// A resonant band-pass swept between heel (dark) and toe (bright):
/// by how hard the notes are played (Auto), by itself (Sweep), or held
/// where Position says (Fixed, for automation to rock the pedal).
pub struct Wah {
    sr: f32,
    mode: usize,
    position: f32,
    sensitivity: f32,
    rate_hz: f32,
    resonance: f32,
    mix: f32,
    phase: f32,
    envelope: f32,
    attack: f32,
    release: f32,
    filters: [Svf; 2],
}

impl Wah {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            mode: 0,
            position: 0.5,
            sensitivity: 0.6,
            rate_hz: 1.5,
            resonance: 0.7,
            mix: 1.0,
            phase: 0.0,
            envelope: 0.0,
            attack: one_pole_coef(0.004, sr),
            release: one_pole_coef(0.15, sr),
            filters: [Svf::new(), Svf::new()],
        }
    }
}

impl EffectProcessor for Wah {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Wah, index) {
            "mode" => self.mode = value.round().max(0.0) as usize,
            "position" => self.position = value.clamp(0.0, 1.0),
            "sensitivity" => self.sensitivity = value.clamp(0.0, 1.0),
            "rate_hz" => self.rate_hz = value.max(0.0),
            "resonance" => self.resonance = value.clamp(0.0, 1.0),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.filters.iter_mut().for_each(Svf::reset);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        use std::f32::consts::TAU;
        // Resonance 0..1 maps to a moderately to very peaky filter; the
        // band-pass output is scaled back to unity at its peak.
        let res = 0.55 + 0.4 * self.resonance;
        let k = 2.0 - 1.95 * res;
        let inc = self.rate_hz / self.sr;
        let [fl, fr] = &mut self.filters;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = 0.5 * (l.abs() + r.abs());
            let coef = if level > self.envelope {
                self.attack
            } else {
                self.release
            };
            self.envelope = level + (self.envelope - level) * coef;
            let pos = match self.mode {
                // Louder playing opens the pedal: about 50 dB of range,
                // shifted by Sensitivity.
                0 => {
                    let db = 20.0 * self.envelope.max(1e-6).log10();
                    ((db + 30.0 + 30.0 * self.sensitivity) / 30.0).clamp(0.0, 1.0)
                }
                1 => 0.5 - 0.5 * (self.phase * TAU).cos(),
                _ => self.position,
            };
            self.phase = (self.phase + inc).fract();
            let hz = HEEL_HZ * (TOE_HZ / HEEL_HZ).powf(pos);
            fl.set(hz, res, self.sr);
            fr.set(hz, res, self.sr);
            let wl = fl.process(*l, FilterMode::BandPass) * k;
            let wr = fr.process(*r, FilterMode::BandPass) * k;
            *l = *l * (1.0 - self.mix) + wl * self.mix;
            *r = *r * (1.0 - self.mix) + wr * self.mix;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    fn through(params: &[(&str, f64)], input: &[f32]) -> f32 {
        let mut fx = with(EffectKind::Wah, params);
        let (l, _) = run(fx.as_mut(), input);
        rms(&l[4_800..]) / rms(&input[4_800..])
    }

    #[test]
    fn heel_is_dark_and_toe_is_bright() {
        let low = sine(400.0, 0.3, 0.4);
        let high = sine(2_000.0, 0.3, 0.4);
        let heel = [("mode", 2.0), ("position", 0.0)];
        let toe = [("mode", 2.0), ("position", 1.0)];
        assert!(through(&heel, &low) > through(&heel, &high) * 3.0);
        assert!(through(&toe, &high) > through(&toe, &low) * 3.0);
    }

    #[test]
    fn auto_mode_opens_up_when_played_harder() {
        let bright = |amp: f32| through(&[("mode", 0.0)], &sine(2_000.0, amp, 0.4));
        assert!(
            bright(0.5) > bright(0.005) * 2.0,
            "{} vs {}",
            bright(0.5),
            bright(0.005)
        );
    }
}
