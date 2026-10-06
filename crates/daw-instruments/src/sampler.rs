//! The sampler: plays an SFZ sample pack (a recorded piano, bass, ...).
//!
//! The pack loads in the background; until it is ready the sampler is
//! silent. Pitch comes from playing each recording faster or slower than
//! recorded (linear interpolation), so keys between sampled notes and
//! sample rates other than the engine's both work.

use daw_sampler::{PackSlot, SamplePack};

use crate::InstrumentProcessor;

const MAX_VOICES: usize = 48;
/// Notes that start at full level still ramp in this fast, so cut-off
/// recordings never click.
const MIN_ATTACK_S: f32 = 0.001;
/// Release when the pack doesn't set one.
const DEFAULT_RELEASE_S: f32 = 0.3;
/// A releasing voice is freed below this level (-80 dB).
const SILENT: f32 = 1e-4;

// Parameter indices (daw_model::instrument::SAMPLER_PARAMS order).
const P_GAIN_DB: usize = 0;
const P_TUNE_CENTS: usize = 1;
const P_TRANSPOSE: usize = 2;
const P_ATTACK_S: usize = 3;
const P_RELEASE_SCALE: usize = 4;
const P_VELOCITY: usize = 5;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    zone: usize,
    note: u8,
    /// Position in the recording, in frames.
    pos: f64,
    /// Semitones from the recording's own pitch (before tuning and bend).
    semitones: f32,
    gain: f32,
    env: f32,
    attack_step: f32,
    releasing: bool,
    release_coef: f32,
    /// Key up while the sustain pedal holds it.
    pedal_held: bool,
    /// The pack's release time for this recording, and whether it ignores
    /// key-up (one-shot).
    release_s: f32,
    one_shot: bool,
    age: u64,
}

pub struct Sampler {
    sample_rate_hz: f32,
    pack: Option<PackSlot>,
    voices: Box<[Voice]>,
    gain: f32,
    tune_cents: f32,
    transpose: f32,
    attack_s: f32,
    release_scale: f32,
    velocity_amount: f32,
    bend_semitones: f32,
    sustain: bool,
    clock: u64,
}

impl Sampler {
    /// A sampler playing `pack` (None: silent until a pack is chosen).
    pub fn new(sample_rate_hz: f32, pack: Option<PackSlot>) -> Self {
        Self {
            sample_rate_hz,
            pack,
            voices: vec![Voice::default(); MAX_VOICES].into_boxed_slice(),
            gain: 1.0,
            tune_cents: 0.0,
            transpose: 0.0,
            attack_s: 0.0,
            release_scale: 1.0,
            velocity_amount: 1.0,
            bend_semitones: 0.0,
            sustain: false,
            clock: 0,
        }
    }

    // RT-SAFE
    fn loaded(&self) -> Option<&SamplePack> {
        match self.pack.as_ref()?.get()? {
            Ok(p) => Some(p),
            Err(_) => None,
        }
    }

    // RT-SAFE
    fn release(v: &mut Voice, sample_rate_hz: f32, scale: f32) {
        let seconds = (v.release_s * scale).max(0.005);
        // Fall 60 dB over the release time.
        v.release_coef = (-6.9 / (seconds * sample_rate_hz)).exp();
        v.releasing = true;
    }
}

impl InstrumentProcessor for Sampler {
    // RT-SAFE
    fn note_on(&mut self, note: u8, velocity: f32) {
        let vel = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
        let (sr, attack_param) = (self.sample_rate_hz, self.attack_s);
        let velocity_amount = self.velocity_amount;
        self.clock += 1;
        let clock = self.clock;
        let Some(pack) = self.loaded() else {
            return;
        };
        // Up to two layered regions per note (most packs use one).
        let mut matched = [usize::MAX; 2];
        let mut n = 0;
        for (i, z) in pack.zones.iter().enumerate() {
            if n < 2 && z.matches(note, vel) {
                matched[n] = i;
                n += 1;
            }
        }
        let mut starts = [(0usize, 0f32, 0f32, 0f32, 0f64, 0f32, false); 2];
        for (k, &zi) in matched[..n].iter().enumerate() {
            let z = &pack.zones[zi];
            let r = &z.region;
            let v = f32::from(vel) / 127.0;
            let amount = velocity_amount * (r.veltrack_percent / 100.0);
            let vel_gain = (1.0 - amount) + amount * v * v;
            let gain = 10f32.powf(r.volume_db / 20.0) * vel_gain;
            let semitones = f32::from(note) - f32::from(r.keycenter) + r.tune_cents / 100.0;
            let attack = r
                .attack_s
                .unwrap_or(0.0)
                .max(attack_param)
                .max(MIN_ATTACK_S);
            starts[k] = (
                zi,
                gain,
                semitones,
                1.0 / (attack * sr),
                r.offset_frames as f64,
                r.release_s.unwrap_or(DEFAULT_RELEASE_S),
                r.one_shot,
            );
        }
        for &(zone, gain, semitones, attack_step, pos, release_s, one_shot) in &starts[..n] {
            // A free voice, else the oldest one.
            let slot = match self.voices.iter().position(|v| !v.active) {
                Some(i) => i,
                None => self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                    .map_or(0, |(i, _)| i),
            };
            if let Some(v) = self.voices.get_mut(slot) {
                *v = Voice {
                    active: true,
                    zone,
                    note,
                    pos,
                    semitones,
                    gain,
                    env: 0.0,
                    attack_step,
                    releasing: false,
                    release_coef: 1.0,
                    pedal_held: false,
                    release_s,
                    one_shot,
                    age: clock,
                };
            }
        }
    }

    // RT-SAFE
    fn note_off(&mut self, note: u8) {
        let (sr, scale, sustain) = (self.sample_rate_hz, self.release_scale, self.sustain);
        for v in self.voices.iter_mut() {
            if !(v.active && v.note == note && !v.releasing) || v.one_shot {
                continue;
            }
            if sustain {
                v.pedal_held = true;
            } else {
                Self::release(v, sr, scale);
            }
        }
    }

    // RT-SAFE
    fn all_notes_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
        self.sustain = false;
    }

    // RT-SAFE
    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            P_GAIN_DB => self.gain = 10f32.powf(value / 20.0),
            P_TUNE_CENTS => self.tune_cents = value,
            P_TRANSPOSE => self.transpose = value,
            P_ATTACK_S => self.attack_s = value.max(0.0),
            P_RELEASE_SCALE => self.release_scale = value.max(0.01),
            P_VELOCITY => self.velocity_amount = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    // RT-SAFE
    fn control_change(&mut self, controller: u8, value: u8) {
        if controller == 64 {
            self.sustain = value >= 64;
            if !self.sustain {
                let (sr, scale) = (self.sample_rate_hz, self.release_scale);
                for v in self.voices.iter_mut().filter(|v| v.active && v.pedal_held) {
                    v.pedal_held = false;
                    Self::release(v, sr, scale);
                }
            }
        }
    }

    // RT-SAFE
    fn pitch_bend(&mut self, semitones: f32) {
        self.bend_semitones = semitones;
    }

    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate_hz;
        let shift = self.transpose + self.tune_cents / 100.0 + self.bend_semitones;
        let master = self.gain;
        let Some(pack) = (match self.pack.as_ref().and_then(|p| p.get()) {
            Some(Ok(p)) => Some(p),
            _ => None,
        }) else {
            return;
        };
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let Some(z) = pack.zones.get(v.zone) else {
                v.active = false;
                continue;
            };
            let frames = z.frames();
            let ch = z.channels.max(1);
            let step =
                f64::from(2f32.powf((v.semitones + shift) / 12.0) * z.sample_rate_hz as f32 / sr);
            let looping = z
                .region
                .loop_frames
                .filter(|(s, e)| (*e as usize) < frames && s < e);
            let scale = 1.0 / 32768.0 * v.gain * master;
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                if let Some((ls, le)) = looping
                    && v.pos > le as f64
                {
                    v.pos -= (le - ls + 1) as f64;
                }
                let i = v.pos as usize;
                if i + 1 >= frames {
                    v.active = false;
                    break;
                }
                let frac = (v.pos - i as f64) as f32;
                let at = |frame: usize, c: usize| {
                    f32::from(z.data.get(frame * ch + c.min(ch - 1)).copied().unwrap_or(0))
                };
                let sl = at(i, 0) + (at(i + 1, 0) - at(i, 0)) * frac;
                let sr_ = at(i, 1) + (at(i + 1, 1) - at(i, 1)) * frac;
                if v.releasing {
                    v.env *= v.release_coef;
                    if v.env < SILENT {
                        v.active = false;
                        break;
                    }
                } else if v.env < 1.0 {
                    v.env = (v.env + v.attack_step).min(1.0);
                }
                let g = scale * v.env;
                *l += sl * g;
                *r += sr_ * g;
                v.pos += step;
            }
        }
    }
}
