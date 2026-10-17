use daw_dsp::{Biquad, BiquadShape};
use daw_model::EffectKind;

use crate::overdrive::warm_clip;
use crate::oversample::Oversampler;
use crate::{EffectProcessor, db_to_gain, param_id};

/// Most gain stages any channel uses.
const STAGES: usize = 3;
/// Most filters in a speaker cabinet.
const CAB_FILTERS: usize = 5;

/// A guitar amp: preamp gain stages (one for Clean, two for Crunch, three
/// for High gain, with the bass tightened between them), a bass/mid/treble
/// tone stack, a gently saturating power amp, and a speaker cabinet.
pub struct Amp {
    sr: f32,
    channel: usize,
    gain: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    master: f32,
    cabinet: usize,
    sides: [Side; 2],
}

#[derive(Clone)]
struct Side {
    input_hp: Biquad,
    stages: [Oversampler; STAGES],
    /// High-pass between stages: keeps stacked gain from turning to mud.
    tighten: [Biquad; STAGES],
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    cab: [Biquad; CAB_FILTERS],
    cab_len: usize,
    dc: Biquad,
}

/// A speaker cabinet as filters: (shape, frequency, Q).
type Cab = &'static [(BiquadShape, f32, f32)];

const CABINETS: [Cab; 4] = [
    // Off: no speaker (straight into the desk).
    &[],
    // 1x12 open back: airy, loose low end.
    &[
        (BiquadShape::HighPass, 70.0, 0.7),
        (BiquadShape::Peak { gain_db: 3.0 }, 120.0, 1.0),
        (BiquadShape::Peak { gain_db: 3.0 }, 2_500.0, 1.0),
        (BiquadShape::LowPass, 5_500.0, 0.7),
        (BiquadShape::LowPass, 6_500.0, 0.7),
    ],
    // 2x12: balanced.
    &[
        (BiquadShape::HighPass, 80.0, 0.8),
        (BiquadShape::Peak { gain_db: 4.0 }, 110.0, 1.2),
        (BiquadShape::Peak { gain_db: 2.0 }, 2_000.0, 1.0),
        (BiquadShape::LowPass, 5_000.0, 0.7),
        (BiquadShape::LowPass, 5_500.0, 0.8),
    ],
    // 4x12 closed back: tight thump, darker top.
    &[
        (BiquadShape::HighPass, 90.0, 1.0),
        (BiquadShape::Peak { gain_db: 5.0 }, 100.0, 1.4),
        (BiquadShape::Peak { gain_db: 2.0 }, 1_600.0, 0.9),
        (BiquadShape::LowPass, 4_500.0, 0.8),
        (BiquadShape::LowPass, 5_000.0, 0.8),
    ],
];

impl Amp {
    pub fn new(sr: f32) -> Self {
        let mut input_hp = Biquad::new();
        input_hp.set(BiquadShape::HighPass, 60.0, 0.7, sr);
        let mut dc = Biquad::new();
        dc.set(BiquadShape::HighPass, 20.0, 0.7, sr);
        let tighten = [100.0, 250.0, 400.0].map(|hz| {
            let mut b = Biquad::new();
            b.set(BiquadShape::HighPass, hz, 0.6, sr);
            b
        });
        let side = Side {
            input_hp,
            stages: std::array::from_fn(|_| Oversampler::new(sr)),
            tighten,
            bass: Biquad::new(),
            mid: Biquad::new(),
            treble: Biquad::new(),
            cab: std::array::from_fn(|_| Biquad::new()),
            cab_len: 0,
            dc,
        };
        let mut a = Self {
            sr,
            channel: 1,
            gain: 5.0,
            bass: 5.0,
            mid: 5.0,
            treble: 5.0,
            master: db_to_gain(-6.0),
            cabinet: 2,
            sides: [side.clone(), side],
        };
        a.set_tone();
        a.set_cabinet();
        a
    }

    // RT-SAFE
    fn set_tone(&mut self) {
        // Knobs 0-10 with 5 flat; each side of 5 is up to 12 dB.
        let db = |knob: f32| (knob - 5.0) * 2.4;
        for s in &mut self.sides {
            s.bass.set(
                BiquadShape::LowShelf {
                    gain_db: db(self.bass),
                },
                120.0,
                0.7,
                self.sr,
            );
            s.mid.set(
                BiquadShape::Peak {
                    gain_db: db(self.mid),
                },
                700.0,
                0.7,
                self.sr,
            );
            s.treble.set(
                BiquadShape::HighShelf {
                    gain_db: db(self.treble),
                },
                3_000.0,
                0.7,
                self.sr,
            );
        }
    }

    // RT-SAFE
    fn set_cabinet(&mut self) {
        let cab = CABINETS[self.cabinet.min(CABINETS.len() - 1)];
        for s in &mut self.sides {
            s.cab_len = cab.len();
            for (f, &(shape, hz, q)) in s.cab.iter_mut().zip(cab) {
                f.set(shape, hz, q, self.sr);
            }
        }
    }
}

impl EffectProcessor for Amp {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Amp, index) {
            "channel" => self.channel = (value.round().max(0.0) as usize).min(2),
            "gain" => self.gain = value.clamp(0.0, 10.0),
            "bass" => {
                self.bass = value.clamp(0.0, 10.0);
                self.set_tone();
            }
            "mid" => {
                self.mid = value.clamp(0.0, 10.0);
                self.set_tone();
            }
            "treble" => {
                self.treble = value.clamp(0.0, 10.0);
                self.set_tone();
            }
            "master_db" => self.master = db_to_gain(value),
            "cabinet" => {
                self.cabinet = value.round().max(0.0) as usize;
                self.set_cabinet();
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        for s in &mut self.sides {
            s.input_hp.reset();
            s.stages.iter_mut().for_each(Oversampler::reset);
            s.tighten.iter_mut().for_each(Biquad::reset);
            s.bass.reset();
            s.mid.reset();
            s.treble.reset();
            s.cab.iter_mut().for_each(Biquad::reset);
            s.dc.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let g = self.gain / 10.0;
        // (stages, gain per stage in dB, output trim): each channel ends up
        // at a similar loudness.
        let (stages, stage_db, trim) = match self.channel {
            0 => (1, 0.0 + 18.0 * g, 0.9),
            1 => (2, 6.0 + 16.0 * g, 0.5),
            _ => (3, 10.0 + 16.0 * g, 0.35),
        };
        let stage_gain = db_to_gain(stage_db);
        let master = self.master * trim;
        let [sl, sr] = &mut self.sides;
        for (buf, s) in [(left, sl), (right, sr)] {
            for x in buf.iter_mut() {
                let mut v = s.input_hp.process(*x);
                for k in 0..stages {
                    v = s.tighten[k].process(v) * stage_gain;
                    v = s.stages[k].process(v, warm_clip);
                }
                v = s
                    .treble
                    .process(s.mid.process(s.bass.process(s.dc.process(v))));
                // The power amp: a little more squash when pushed.
                v = (v * 1.5).tanh() / 1.5;
                for f in &mut s.cab[..s.cab_len] {
                    v = f.process(v);
                }
                *x = v * master;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    fn harmonics(channel: f64) -> f32 {
        let mut fx = with(
            EffectKind::Amp,
            &[("channel", channel), ("cabinet", 0.0), ("gain", 6.0)],
        );
        let (l, _) = run(fx.as_mut(), &sine(196.0, 0.3, 0.5));
        let tail = &l[6_000..];
        assert!(peak(tail) < 1.5, "channel {channel}: {}", peak(tail));
        (level_at(tail, 392.0) + level_at(tail, 588.0)) / level_at(tail, 196.0)
    }

    #[test]
    fn channels_go_from_clean_to_high_gain() {
        let (clean, crunch, high) = (harmonics(0.0), harmonics(1.0), harmonics(2.0));
        assert!(clean < crunch && crunch < high, "{clean} {crunch} {high}");
    }

    #[test]
    fn a_cabinet_rolls_off_the_fizz() {
        let fizz = |cabinet: f64| {
            let mut fx = with(
                EffectKind::Amp,
                &[("channel", 0.0), ("gain", 2.0), ("cabinet", cabinet)],
            );
            let (l, _) = run(fx.as_mut(), &sine(9_000.0, 0.2, 0.3));
            rms(&l[4_000..])
        };
        assert!(
            fizz(3.0) < fizz(0.0) * 0.2,
            "{} vs {}",
            fizz(3.0),
            fizz(0.0)
        );
    }

    #[test]
    fn the_tone_stack_turns_bass_up_and_down() {
        let lows = |bass: f64| {
            let mut fx = with(
                EffectKind::Amp,
                &[
                    ("channel", 0.0),
                    ("gain", 0.0),
                    ("cabinet", 0.0),
                    ("bass", bass),
                ],
            );
            let (l, _) = run(fx.as_mut(), &sine(100.0, 0.05, 0.4));
            rms(&l[4_000..])
        };
        assert!(
            lows(10.0) > lows(0.0) * 3.0,
            "{} vs {}",
            lows(10.0),
            lows(0.0)
        );
    }
}
