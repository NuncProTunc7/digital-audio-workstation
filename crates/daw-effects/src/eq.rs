use daw_dsp::{Biquad, BiquadShape};
use daw_model::EffectKind;

use crate::{EffectProcessor, param_id};

/// Low cut + low shelf + mid peak + high shelf, per channel.
pub struct Eq {
    sr: f32,
    low_cut_hz: f32,
    low_gain_db: f32,
    low_freq_hz: f32,
    mid_gain_db: f32,
    mid_freq_hz: f32,
    mid_q: f32,
    high_gain_db: f32,
    high_freq_hz: f32,
    // [channel][band]
    filters: [[Biquad; 4]; 2],
}

impl Eq {
    pub fn new(sr: f32) -> Self {
        let mut eq = Self {
            sr,
            low_cut_hz: 20.0,
            low_gain_db: 0.0,
            low_freq_hz: 150.0,
            mid_gain_db: 0.0,
            mid_freq_hz: 1_000.0,
            mid_q: 1.0,
            high_gain_db: 0.0,
            high_freq_hz: 6_000.0,
            filters: Default::default(),
        };
        eq.update();
        eq
    }

    // RT-SAFE
    fn update(&mut self) {
        for ch in &mut self.filters {
            ch[0].set(BiquadShape::HighPass, self.low_cut_hz, 0.707, self.sr);
            ch[1].set(
                BiquadShape::LowShelf {
                    gain_db: self.low_gain_db,
                },
                self.low_freq_hz,
                0.707,
                self.sr,
            );
            ch[2].set(
                BiquadShape::Peak {
                    gain_db: self.mid_gain_db,
                },
                self.mid_freq_hz,
                self.mid_q,
                self.sr,
            );
            ch[3].set(
                BiquadShape::HighShelf {
                    gain_db: self.high_gain_db,
                },
                self.high_freq_hz,
                0.707,
                self.sr,
            );
        }
    }
}

impl EffectProcessor for Eq {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Eq, index) {
            "low_cut_hz" => self.low_cut_hz = value,
            "low.gain_db" => self.low_gain_db = value,
            "low.freq_hz" => self.low_freq_hz = value,
            "mid.gain_db" => self.mid_gain_db = value,
            "mid.freq_hz" => self.mid_freq_hz = value,
            "mid.q" => self.mid_q = value,
            "high.gain_db" => self.high_gain_db = value,
            "high.freq_hz" => self.high_freq_hz = value,
            _ => return,
        }
        self.update();
    }

    fn reset(&mut self) {
        for ch in &mut self.filters {
            for f in ch {
                f.reset();
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // At 20 Hz the low cut is inaudible; skip it to save CPU.
        let cut = self.low_cut_hz > 21.0;
        let [fl, fr] = &mut self.filters;
        for (buf, filters) in [(left, fl), (right, fr)] {
            for s in buf.iter_mut() {
                let mut x = *s;
                if cut {
                    x = filters[0].process(x);
                }
                for f in &mut filters[1..] {
                    x = f.process(x);
                }
                *s = x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn flat_by_default() {
        let mut fx = with(EffectKind::Eq, &[]);
        let input = sine(440.0, 0.5, 0.5);
        let (l, _) = run(fx.as_mut(), &input);
        let err = l
            .iter()
            .zip(&input)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-3, "{err}");
    }

    #[test]
    fn bass_boost_and_low_cut() {
        let mut boost = with(EffectKind::Eq, &[("low.gain_db", 12.0)]);
        let input = sine(60.0, 0.1, 1.0);
        let (l, _) = run(boost.as_mut(), &input);
        assert!(rms(&l[24_000..]) / rms(&input[24_000..]) > 3.5);

        let mut cut = with(EffectKind::Eq, &[("low_cut_hz", 200.0)]);
        let rumble = sine(30.0, 0.5, 1.0);
        let (l, _) = run(cut.as_mut(), &rumble);
        assert!(rms(&l[24_000..]) < 0.02);
    }
}
