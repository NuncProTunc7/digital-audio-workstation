use daw_model::EffectKind;

use crate::{EffectProcessor, db_to_gain, param_id};

/// Soft-clipping saturation followed by a tone (low-pass) control.
pub struct Distortion {
    sr: f32,
    drive: f32,
    output: f32,
    mix: f32,
    tone_coef: f32,
    state: [f32; 2],
}

impl Distortion {
    pub fn new(sr: f32) -> Self {
        let mut d = Self {
            sr,
            drive: db_to_gain(12.0),
            output: db_to_gain(-6.0),
            mix: 1.0,
            tone_coef: 0.0,
            state: [0.0; 2],
        };
        d.set_tone(8_000.0);
        d
    }

    // RT-SAFE
    fn set_tone(&mut self, hz: f32) {
        let x = (-std::f32::consts::TAU * hz.min(self.sr * 0.45) / self.sr).exp();
        self.tone_coef = x;
    }
}

impl EffectProcessor for Distortion {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Distortion, index) {
            "drive_db" => self.drive = db_to_gain(value),
            "tone_hz" => self.set_tone(value),
            "output_db" => self.output = db_to_gain(value),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.state = [0.0; 2];
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (drive, out, mix, c) = (self.drive, self.output, self.mix, self.tone_coef);
        let [sl, sr] = &mut self.state;
        for (buf, state) in [(left, sl), (right, sr)] {
            for s in buf.iter_mut() {
                let shaped = (*s * drive).tanh();
                *state = shaped + (*state - shaped) * c;
                *s = *s * (1.0 - mix) + *state * out * mix;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn adds_harmonics_and_stays_bounded() {
        let mut fx = with(
            EffectKind::Distortion,
            &[
                ("drive_db", 30.0),
                ("output_db", 0.0),
                ("tone_hz", 20_000.0),
            ],
        );
        let (l, _) = run(fx.as_mut(), &sine(100.0, 0.5, 0.5));
        assert!(peak(&l) <= 1.0);
        // A heavily driven sine becomes square-ish: RMS close to peak.
        let tail = &l[12_000..];
        assert!(rms(tail) / peak(tail) > 0.85, "{}", rms(tail) / peak(tail));
    }
}
