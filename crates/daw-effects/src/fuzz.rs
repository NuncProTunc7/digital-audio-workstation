use daw_dsp::{Biquad, BiquadShape};
use daw_model::EffectKind;

use crate::oversample::Oversampler;
use crate::{EffectProcessor, db_to_gain, param_id};

/// A big, buzzy fuzz (the Big Muff idea): huge gain into near-square
/// clipping, then a tone control that blends a dark and a bright path,
/// scooping the mids in between.
pub struct Fuzz {
    fuzz: f32,
    tone: f32,
    level: f32,
    channels: [Channel; 2],
}

#[derive(Clone)]
struct Channel {
    pre_hp: Biquad,
    clip: Oversampler,
    dark: Biquad,
    bright: Biquad,
    dc: Biquad,
}

/// Hard, square-ish clipping with rounded corners.
#[inline]
fn fuzz_clip(x: f32) -> f32 {
    let y = x / (1.0 + x.abs().powi(4)).powf(0.25);
    // A touch of asymmetry, for the transistor bite.
    if y > 0.0 { y } else { y * 0.85 }
}

impl Fuzz {
    pub fn new(sr: f32) -> Self {
        let mut pre_hp = Biquad::new();
        pre_hp.set(BiquadShape::HighPass, 90.0, 0.7, sr);
        let mut dark = Biquad::new();
        dark.set(BiquadShape::LowPass, 450.0, 0.7, sr);
        let mut bright = Biquad::new();
        bright.set(BiquadShape::HighPass, 1_100.0, 0.7, sr);
        let mut dc = Biquad::new();
        dc.set(BiquadShape::HighPass, 20.0, 0.7, sr);
        let ch = Channel {
            pre_hp,
            clip: Oversampler::new(sr),
            dark,
            bright,
            dc,
        };
        Self {
            fuzz: 0.7,
            tone: 0.5,
            level: db_to_gain(-8.0),
            channels: [ch.clone(), ch],
        }
    }
}

impl EffectProcessor for Fuzz {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Fuzz, index) {
            "fuzz" => self.fuzz = value.clamp(0.0, 1.0),
            "tone" => self.tone = value.clamp(0.0, 1.0),
            "level_db" => self.level = db_to_gain(value),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for c in &mut self.channels {
            c.pre_hp.reset();
            c.clip.reset();
            c.dark.reset();
            c.bright.reset();
            c.dc.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // 20 to 60 dB of gain.
        let gain = db_to_gain(20.0 + 40.0 * self.fuzz);
        let (tone, level) = (self.tone, self.level);
        let [cl, cr] = &mut self.channels;
        for (buf, c) in [(left, cl), (right, cr)] {
            for s in buf.iter_mut() {
                let x = c.pre_hp.process(*s) * gain;
                let y = c.dc.process(c.clip.process(x, fuzz_clip));
                // The bright path is quieter by nature; lift it to match.
                let shaped = c.dark.process(y) * (1.0 - tone) + c.bright.process(y) * tone * 1.4;
                *s = shaped * level;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn a_sine_becomes_a_buzzy_square() {
        let mut fx = with(
            EffectKind::Fuzz,
            &[("fuzz", 1.0), ("tone", 0.5), ("level_db", 0.0)],
        );
        let (l, _) = run(fx.as_mut(), &sine(110.0, 0.3, 0.5));
        let tail = &l[6_000..];
        assert!(peak(tail) < 2.0, "{}", peak(tail));
        // Strong odd harmonics, as a square wave has.
        let third = level_at(tail, 330.0) / level_at(tail, 110.0);
        assert!(third > 0.15, "3rd harmonic {third}");
    }

    #[test]
    fn tone_moves_from_dark_to_bright() {
        let input = sine(220.0, 0.3, 0.5);
        let brightness = |tone: f64| {
            let mut fx = with(EffectKind::Fuzz, &[("tone", tone)]);
            let (l, _) = run(fx.as_mut(), &input);
            let tail = &l[6_000..];
            level_at(tail, 2_420.0) / level_at(tail, 220.0)
        };
        assert!(brightness(1.0) > brightness(0.0) * 3.0);
    }
}
