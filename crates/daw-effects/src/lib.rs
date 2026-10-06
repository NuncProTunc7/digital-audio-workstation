//! Built-in audio effects.
//!
//! Effects are created off the audio thread (they allocate their delay lines
//! up front) and then run on it through [`EffectProcessor`], whose methods
//! never allocate, lock, or do I/O.

mod chorus;
mod compressor;
mod delay;
mod distortion;
mod eq;
mod limiter;
mod reverb;

use daw_model::effect::{Effect, EffectKind, effect_params};

/// The real-time face of an effect. Processes stereo audio in place.
pub trait EffectProcessor: Send {
    /// Sets a parameter by its index in `daw_model::effect::effect_params`.
    // RT-SAFE
    fn set_param(&mut self, index: usize, value: f32);
    /// Tempo, for effects that sync to the beat.
    // RT-SAFE
    fn set_tempo(&mut self, _bpm: f32) {}
    /// Clears internal state (echo and reverb tails).
    // RT-SAFE
    fn reset(&mut self);
    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
}

/// Builds the processor for an effect's settings. Allocates; call off the
/// audio thread.
pub fn create(effect: &Effect, sample_rate_hz: f32) -> Box<dyn EffectProcessor> {
    let sr = sample_rate_hz.max(1.0);
    let mut processor: Box<dyn EffectProcessor> = match effect.kind {
        EffectKind::Eq => Box::new(eq::Eq::new(sr)),
        EffectKind::Compressor => Box::new(compressor::Compressor::new(sr)),
        EffectKind::Reverb => Box::new(reverb::Reverb::new(sr)),
        EffectKind::Delay => Box::new(delay::Delay::new(sr)),
        EffectKind::Chorus => Box::new(chorus::Chorus::new(sr)),
        EffectKind::Distortion => Box::new(distortion::Distortion::new(sr)),
        EffectKind::Limiter => Box::new(limiter::Limiter::new(sr)),
    };
    for (index, spec) in effect_params(effect.kind).iter().enumerate() {
        processor.set_param(index, effect.value(spec.id).unwrap_or(spec.default) as f32);
    }
    processor
}

/// Index of a parameter id, for effect implementations.
pub(crate) fn param_id(kind: EffectKind, index: usize) -> &'static str {
    effect_params(kind).get(index).map_or("", |s| s.id)
}

/// Per-sample multiplier that moves 63% of the way to a target in `time_s`.
pub(crate) fn one_pole_coef(time_s: f32, sample_rate_hz: f32) -> f32 {
    (-1.0 / (time_s.max(1e-5) * sample_rate_hz)).exp()
}

pub(crate) fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A fixed-size circular buffer with fractional-delay reads.
#[derive(Debug, Clone)]
pub(crate) struct DelayLine {
    buffer: Box<[f32]>,
    write: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_samples.max(2) + 2].into_boxed_slice(),
            write: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
    }

    // RT-SAFE
    pub fn push(&mut self, x: f32) {
        self.write = (self.write + 1) % self.buffer.len();
        self.buffer[self.write] = x;
    }

    /// Reads `delay` samples back (linear interpolation), clamped to the
    /// buffer length.
    // RT-SAFE
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buffer.len();
        let d = delay.clamp(0.0, (len - 2) as f32);
        let whole = d as usize;
        let frac = d - whole as f32;
        let a = self.buffer[(self.write + len - whole) % len];
        let b = self.buffer[(self.write + len - whole - 1) % len];
        a + (b - a) * frac
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    pub const SR: f32 = 48_000.0;

    pub fn sine(freq_hz: f32, amp: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * SR) as usize)
            .map(|i| (std::f32::consts::TAU * freq_hz * i as f32 / SR).sin() * amp)
            .collect()
    }

    pub fn run(fx: &mut dyn EffectProcessor, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b);
        }
        (l, r)
    }

    pub fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    pub fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    pub fn with(kind: EffectKind, params: &[(&str, f64)]) -> Box<dyn EffectProcessor> {
        let mut e = Effect::new(1, kind);
        for (k, v) in params {
            e.params.insert((*k).to_owned(), *v);
        }
        create(&e, SR)
    }

    /// Every effect at every parameter extreme stays finite and bounded.
    #[test]
    fn all_effects_survive_extreme_settings() {
        for kind in EffectKind::ALL {
            for pick_max in [false, true] {
                let mut e = Effect::new(1, kind);
                for s in effect_params(kind) {
                    e.params
                        .insert(s.id.to_owned(), if pick_max { s.max } else { s.min });
                }
                let mut fx = create(&e, SR);
                fx.set_tempo(300.0);
                let mut noise = daw_dsp::Rng::new(3);
                let input: Vec<f32> = (0..(SR as usize)).map(|_| noise.next_bipolar()).collect();
                let (l, r) = run(fx.as_mut(), &input);
                for s in l.iter().chain(&r) {
                    assert!(s.is_finite(), "{kind:?} max={pick_max} produced {s}");
                    assert!(s.abs() < 20.0, "{kind:?} max={pick_max} produced {s}");
                }
            }
        }
    }

    #[test]
    fn delay_line_reads_back_exact_and_fractional_delays() {
        let mut d = DelayLine::new(100);
        for i in 0..50 {
            d.push(i as f32);
        }
        assert_eq!(d.read(0.0), 49.0);
        assert_eq!(d.read(10.0), 39.0);
        assert!((d.read(10.5) - 38.5).abs() < 1e-6);
    }
}
