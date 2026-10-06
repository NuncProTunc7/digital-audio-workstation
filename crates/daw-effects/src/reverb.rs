//! Freeverb-style reverb: eight parallel damped comb filters into four
//! series all-pass filters per channel. Based on the public-domain Freeverb
//! design by Jezar at Dreampoint; implemented from scratch.

use daw_model::EffectKind;

use crate::{DelayLine, EffectProcessor, param_id};

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const TUNING_RATE_HZ: f32 = 44_100.0;
const INPUT_GAIN: f32 = 0.015;
const WET_SCALE: f32 = 3.0;
const MAX_PREDELAY_S: f32 = 0.25;

#[derive(Debug, Clone)]
struct Comb {
    buffer: Box<[f32]>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)].into_boxed_slice(),
            pos: 0,
            store: 0.0,
        }
    }

    // RT-SAFE
    fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buffer[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buffer[self.pos] = input + self.store * feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.store = 0.0;
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buffer: Box<[f32]>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)].into_boxed_slice(),
            pos: 0,
        }
    }

    // RT-SAFE
    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        self.buffer[self.pos] = input + buffered * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        buffered - input
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

pub struct Reverb {
    sr: f32,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    predelay: [DelayLine; 2],
    predelay_samples: f32,
    feedback: f32,
    damp: f32,
    width: f32,
    mix: f32,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let scale = sr / TUNING_RATE_HZ;
        let build = |spread: usize| {
            (
                COMB_TUNING
                    .iter()
                    .map(|&t| Comb::new(((t + spread) as f32 * scale) as usize))
                    .collect::<Vec<_>>(),
                ALLPASS_TUNING
                    .iter()
                    .map(|&t| Allpass::new(((t + spread) as f32 * scale) as usize))
                    .collect::<Vec<_>>(),
            )
        };
        let (cl, al) = build(0);
        let (cr, ar) = build(STEREO_SPREAD);
        let max_pre = (MAX_PREDELAY_S * sr) as usize;
        let mut r = Self {
            sr,
            combs: [cl, cr],
            allpasses: [al, ar],
            predelay: [DelayLine::new(max_pre), DelayLine::new(max_pre)],
            predelay_samples: 0.0,
            feedback: 0.0,
            damp: 0.0,
            width: 1.0,
            mix: 0.25,
        };
        r.set_size(0.6);
        r.set_damping(0.4);
        r
    }

    fn set_size(&mut self, size: f32) {
        self.feedback = size.clamp(0.0, 1.0) * 0.28 + 0.7;
    }

    fn set_damping(&mut self, damping: f32) {
        self.damp = damping.clamp(0.0, 1.0) * 0.4;
    }
}

impl EffectProcessor for Reverb {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Reverb, index) {
            "size" => self.set_size(value),
            "damping" => self.set_damping(value),
            "width" => self.width = value.clamp(0.0, 1.0),
            "predelay_s" => self.predelay_samples = value.clamp(0.0, MAX_PREDELAY_S) * self.sr,
            "mix" => self.mix = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for ch in &mut self.combs {
            ch.iter_mut().for_each(Comb::clear);
        }
        for ch in &mut self.allpasses {
            ch.iter_mut().for_each(Allpass::clear);
        }
        self.predelay.iter_mut().for_each(DelayLine::clear);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wet = self.mix * WET_SCALE;
        let wet1 = wet * (self.width / 2.0 + 0.5);
        let wet2 = wet * ((1.0 - self.width) / 2.0);
        let dry = 1.0 - self.mix;
        let (fb, damp) = (self.feedback, self.damp);
        let [cl, cr] = &mut self.combs;
        let [al, ar] = &mut self.allpasses;
        let [pl, pr] = &mut self.predelay;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = (*l + *r) * INPUT_GAIN;
            pl.push(input);
            pr.push(input);
            let in_l = pl.read(self.predelay_samples);
            let in_r = pr.read(self.predelay_samples);
            let mut out_l: f32 = cl.iter_mut().map(|c| c.process(in_l, fb, damp)).sum();
            let mut out_r: f32 = cr.iter_mut().map(|c| c.process(in_r, fb, damp)).sum();
            for a in al.iter_mut() {
                out_l = a.process(out_l);
            }
            for a in ar.iter_mut() {
                out_r = a.process(out_r);
            }
            let (dl, dr) = (*l, *r);
            *l = dl * dry + out_l * wet1 + out_r * wet2;
            *r = dr * dry + out_r * wet1 + out_l * wet2;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn leaves_a_decaying_tail() {
        let mut fx = with(EffectKind::Reverb, &[("mix", 0.5), ("predelay_s", 0.0)]);
        let mut input = vec![0.0; (SR * 4.0) as usize];
        input[..4_800].copy_from_slice(&sine(440.0, 0.5, 0.1));
        let (l, r) = run(fx.as_mut(), &input);
        let after = |s: f32, e: f32| rms(&l[(s * SR) as usize..(e * SR) as usize]);
        assert!(after(0.2, 0.4) > 0.005, "no tail: {}", after(0.2, 0.4));
        assert!(
            after(3.5, 4.0) < after(0.2, 0.4) / 10.0,
            "tail does not decay"
        );
        // Stereo: channels differ.
        assert!(l.iter().zip(&r).any(|(a, b)| (a - b).abs() > 1e-4));
    }

    #[test]
    fn dry_only_at_zero_mix() {
        let mut fx = with(EffectKind::Reverb, &[("mix", 0.0)]);
        let input = sine(440.0, 0.5, 0.2);
        let (l, _) = run(fx.as_mut(), &input);
        assert_eq!(l, input);
    }
}
