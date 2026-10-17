use daw_model::EffectKind;

use crate::{EffectProcessor, db_to_gain, one_pole_coef, param_id};

/// How far below the threshold the gate waits before closing again, so it
/// doesn't flutter on a note that hovers around it.
const HYSTERESIS_DB: f32 = 6.0;

/// Silences the track while it is quieter than the threshold (hiss and hum
/// between phrases), opening at once when a note starts and closing over
/// Release. Both sides open and close together.
pub struct NoiseGate {
    sr: f32,
    open_at: f32,
    close_at: f32,
    release: f32,
    open_coef: f32,
    detector: f32,
    detect_release: f32,
    open: bool,
    gain: f32,
}

impl NoiseGate {
    pub fn new(sr: f32) -> Self {
        let mut g = Self {
            sr,
            open_at: 0.0,
            close_at: 0.0,
            release: 0.0,
            open_coef: one_pole_coef(0.001, sr),
            detector: 0.0,
            detect_release: one_pole_coef(0.02, sr),
            open: false,
            gain: 0.0,
        };
        g.set_threshold(-55.0);
        g.release = one_pole_coef(0.12, sr);
        g
    }

    // RT-SAFE
    fn set_threshold(&mut self, db: f32) {
        self.open_at = db_to_gain(db);
        self.close_at = db_to_gain(db - HYSTERESIS_DB);
    }
}

impl EffectProcessor for NoiseGate {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::NoiseGate, index) {
            "threshold_db" => self.set_threshold(value),
            "release_s" => self.release = one_pole_coef(value, self.sr),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.detector = 0.0;
        self.open = false;
        self.gain = 0.0;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // Peak detector: instant up, quick fall.
            let level = l.abs().max(r.abs());
            self.detector = level.max(self.detector * self.detect_release);
            if self.detector > self.open_at {
                self.open = true;
            } else if self.detector < self.close_at {
                self.open = false;
            }
            let (target, coef) = if self.open {
                (1.0, self.open_coef)
            } else {
                (0.0, self.release)
            };
            self.gain = target + (self.gain - target) * coef;
            *l *= self.gain;
            *r *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn hiss_is_silenced_and_notes_pass() {
        let mut fx = with(EffectKind::NoiseGate, &[("threshold_db", -50.0)]);
        let mut noise = daw_dsp::Rng::new(9);
        // Half a second of -70 dB hiss, then a loud note.
        let mut input: Vec<f32> = (0..24_000).map(|_| noise.next_bipolar() * 3e-4).collect();
        input.extend(sine(220.0, 0.5, 0.5));
        let (l, _) = run(fx.as_mut(), &input);
        assert!(
            peak(&l[2_400..24_000]) < 1e-5,
            "hiss {}",
            peak(&l[2_400..24_000])
        );
        let note = &l[26_400..];
        assert!(rms(note) > rms(&input[26_400..]) * 0.95);
    }
}
