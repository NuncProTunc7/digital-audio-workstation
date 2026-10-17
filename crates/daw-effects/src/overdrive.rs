use daw_dsp::{Biquad, BiquadShape};
use daw_model::EffectKind;

use crate::oversample::Oversampler;
use crate::{EffectProcessor, db_to_gain, param_id};

/// A tube-screamer-style overdrive (the idea, not a circuit copy): bass is
/// trimmed and the mids pushed before a soft, slightly lopsided clip, so
/// chords stay clear and single notes sing; then a tone control.
pub struct Overdrive {
    sr: f32,
    drive: f32,
    tone: f32,
    level: f32,
    channels: [Channel; 2],
}

#[derive(Clone)]
struct Channel {
    pre_hp: Biquad,
    pre_mid: Biquad,
    clip: Oversampler,
    tone: Biquad,
    dc: Biquad,
}

/// Lopsided soft clipping (as from mismatched diodes): adds the even
/// harmonics that make overdrive sound warm.
#[inline]
pub(crate) fn warm_clip(x: f32) -> f32 {
    const BIAS: f32 = 0.15;
    (x + BIAS).tanh() - BIAS.tanh()
}

impl Overdrive {
    pub fn new(sr: f32) -> Self {
        let mut pre_hp = Biquad::new();
        pre_hp.set(BiquadShape::HighPass, 180.0, 0.7, sr);
        let mut pre_mid = Biquad::new();
        pre_mid.set(BiquadShape::Peak { gain_db: 6.0 }, 720.0, 0.8, sr);
        let mut dc = Biquad::new();
        dc.set(BiquadShape::HighPass, 20.0, 0.7, sr);
        let ch = Channel {
            pre_hp,
            pre_mid,
            clip: Oversampler::new(sr),
            tone: Biquad::new(),
            dc,
        };
        let mut o = Self {
            sr,
            drive: 0.5,
            tone: 0.5,
            level: db_to_gain(-3.0),
            channels: [ch.clone(), ch],
        };
        o.set_tone();
        o
    }

    // RT-SAFE
    fn set_tone(&mut self) {
        // Dark (1.2 kHz) to bright (9 kHz), on a log scale.
        let hz = 1_200.0 * 7.5f32.powf(self.tone);
        for c in &mut self.channels {
            c.tone.set(BiquadShape::LowPass, hz, 0.7, self.sr);
        }
    }
}

impl EffectProcessor for Overdrive {
    fn set_param(&mut self, index: usize, value: f32) {
        match param_id(EffectKind::Overdrive, index) {
            "drive" => self.drive = value.clamp(0.0, 1.0),
            "tone" => {
                self.tone = value.clamp(0.0, 1.0);
                self.set_tone();
            }
            "level_db" => self.level = db_to_gain(value),
            _ => {}
        }
    }

    fn reset(&mut self) {
        for c in &mut self.channels {
            c.pre_hp.reset();
            c.pre_mid.reset();
            c.clip.reset();
            c.tone.reset();
            c.dc.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // 6 to 42 dB of gain into the clipper.
        let gain = db_to_gain(6.0 + 36.0 * self.drive);
        // Keep the output level about the same as the drive goes up.
        let makeup = 0.7;
        let level = self.level;
        let [cl, cr] = &mut self.channels;
        for (buf, c) in [(left, cl), (right, cr)] {
            for s in buf.iter_mut() {
                let x = c.pre_mid.process(c.pre_hp.process(*s)) * gain;
                let y = c.clip.process(x, warm_clip);
                *s = c.tone.process(c.dc.process(y)) * makeup * level;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;
    use daw_model::EffectKind;

    #[test]
    fn more_drive_means_more_harmonics_and_stays_bounded() {
        let input = sine(220.0, 0.3, 0.5);
        let harmonics = |drive: f64| {
            let mut fx = with(
                EffectKind::Overdrive,
                &[("drive", drive), ("tone", 1.0), ("level_db", 0.0)],
            );
            let (l, _) = run(fx.as_mut(), &input);
            let tail = &l[6_000..];
            assert!(peak(tail) < 1.0, "{}", peak(tail));
            // Second and third harmonics against the note itself.
            (level_at(tail, 440.0) + level_at(tail, 660.0)) / level_at(tail, 220.0)
        };
        let (light, heavy) = (harmonics(0.0), harmonics(1.0));
        assert!(heavy > light * 2.0, "{light} vs {heavy}");
        assert!(light > 0.01, "even light drive colours the sound: {light}");
    }

    #[test]
    fn high_notes_do_not_fold_back_as_low_tones() {
        // A 7 kHz note clipped hard: its 5th harmonic (35 kHz) is above what
        // 48 kHz audio can carry, and without oversampling it would fold back
        // to 48 - 35 = 13 kHz.
        let mut fx = with(
            EffectKind::Overdrive,
            &[("drive", 1.0), ("tone", 1.0), ("level_db", 0.0)],
        );
        let (l, _) = run(fx.as_mut(), &sine(7_000.0, 0.5, 0.3));
        let tail = &l[4_000..];
        let folded = level_at(tail, 13_000.0);
        assert!(folded < level_at(tail, 7_000.0) * 0.05, "aliasing {folded}");
    }
}
