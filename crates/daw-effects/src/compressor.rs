use daw_model::EffectKind;

use crate::{EffectProcessor, db_to_gain, one_pole_coef, param_id};

/// Feed-forward, stereo-linked compressor with a soft knee.
pub struct Compressor {
    sr: f32,
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    makeup_gain: f32,
    mix: f32,
    attack_coef: f32,
    release_coef: f32,
    /// Smoothed gain reduction in dB (≤ 0).
    gr_db: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            makeup_gain: 1.0,
            mix: 1.0,
            attack_coef: one_pole_coef(0.01, sr),
            release_coef: one_pole_coef(0.15, sr),
            gr_db: 0.0,
        }
    }

    /// Static curve: how many dB to reduce a signal at `level_db`.
    // RT-SAFE
    fn gain_reduction_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 / self.ratio - 1.0;
        let half_knee = self.knee_db / 2.0;
        if over <= -half_knee {
            0.0
        } else if over >= half_knee || self.knee_db <= 0.0 {
            slope * over
        } else {
            let x = over + half_knee;
            slope * x * x / (2.0 * self.knee_db)
        }
    }
}

impl EffectProcessor for Compressor {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Compressor, index) {
            "threshold_db" => self.threshold_db = value,
            "ratio" => self.ratio = value.max(1.0),
            "attack_s" => self.attack_coef = one_pole_coef(value, self.sr),
            "release_s" => self.release_coef = one_pole_coef(value, self.sr),
            "knee_db" => self.knee_db = value.max(0.0),
            "makeup_db" => self.makeup_gain = db_to_gain(value),
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.gr_db = 0.0;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let peak = l.abs().max(r.abs());
            let g = self.next_gain(peak);
            *l *= g;
            *r *= g;
        }
    }

    // Sidechain: the key's level decides the gain reduction, so the track
    // ducks under, say, the kick.
    fn process_keyed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key_left: &[f32],
        key_right: &[f32],
    ) {
        for ((l, r), (kl, kr)) in left
            .iter_mut()
            .zip(right.iter_mut())
            .zip(key_left.iter().zip(key_right.iter()))
        {
            let g = self.next_gain(kl.abs().max(kr.abs()));
            *l *= g;
            *r *= g;
        }
    }
}

impl Compressor {
    /// The gain for the next sample, given the detector's peak level.
    // RT-SAFE
    fn next_gain(&mut self, peak: f32) -> f32 {
        let target = self.gain_reduction_db(20.0 * peak.max(1e-9).log10());
        // Reducing gain uses attack; recovering uses release.
        let coef = if target < self.gr_db {
            self.attack_coef
        } else {
            self.release_coef
        };
        self.gr_db = target + (self.gr_db - target) * coef;
        let g = db_to_gain(self.gr_db) * self.makeup_gain;
        1.0 - self.mix + self.mix * g
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn reduces_loud_signals_and_leaves_quiet_ones() {
        let mut fx = with(
            EffectKind::Compressor,
            &[("threshold_db", -20.0), ("ratio", 10.0), ("knee_db", 0.0)],
        );
        let loud = sine(200.0, 0.9, 1.0);
        let (l, _) = run(fx.as_mut(), &loud);
        // 0.9 ≈ -0.9 dBFS, 19 dB over: expect roughly 17 dB of reduction.
        assert!(peak(&l[24_000..]) < 0.2, "{}", peak(&l[24_000..]));

        let mut fx = with(
            EffectKind::Compressor,
            &[("threshold_db", -20.0), ("ratio", 10.0)],
        );
        let quiet = sine(200.0, 0.01, 1.0);
        let (l, _) = run(fx.as_mut(), &quiet);
        assert!((peak(&l[24_000..]) - 0.01).abs() < 0.0005);
    }

    #[test]
    fn a_sidechain_key_ducks_a_quiet_signal() {
        let mut fx = with(
            EffectKind::Compressor,
            &[("threshold_db", -20.0), ("ratio", 10.0), ("knee_db", 0.0)],
        );
        let pad = sine(200.0, 0.1, 1.0);
        let (mut l, mut r) = (pad.clone(), pad.clone());
        // The key is loud for the first half and silent for the second.
        let n = l.len();
        let key: Vec<f32> = (0..n).map(|i| if i < n / 2 { 0.9 } else { 0.0 }).collect();
        fx.process_keyed(&mut l, &mut r, &key, &key);
        assert!(
            peak(&l[n / 4..n / 2]) < 0.03,
            "ducked {}",
            peak(&l[n / 4..n / 2])
        );
        assert!(
            peak(&l[n - 4_800..]) > 0.09,
            "recovered {}",
            peak(&l[n - 4_800..])
        );
    }
}
