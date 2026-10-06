//! Synthesized drum machine with 16 pads on General MIDI drum notes 36–51.
//!
//! Every sound is synthesized (no samples), in the spirit of classic analog
//! drum machines: pitched sines for kick and toms, filtered noise for snares
//! and claps, and a bank of detuned square waves for metallic hats and
//! cymbals. Implemented from scratch; no third-party code.

use daw_dsp::{FilterMode, Oscillator, Rng, Smoother, Svf, Waveform, db_to_gain};
use daw_model::instrument::DRUM_PARAMS;
use serde::Serialize;

use crate::InstrumentProcessor;

/// Mixer group a pad's level knob belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrumGroup {
    Kick,
    Snare,
    Hats,
    Toms,
    Perc,
    Cymbals,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sound {
    Kick,
    Rim,
    Snare,
    Clap,
    Snare2,
    /// Resting pitch in hertz.
    Tom(f32),
    ClosedHat,
    PedalHat,
    OpenHat,
    Crash,
    Ride,
}

/// One pad: its MIDI note, name, and level group.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DrumPad {
    pub note: u8,
    pub name: &'static str,
    pub group: DrumGroup,
    #[serde(skip)]
    sound: Sound,
    /// Stereo position, -1.0 (left) to 1.0 (right).
    #[serde(skip)]
    pan: f32,
}

const fn pad(note: u8, name: &'static str, group: DrumGroup, sound: Sound, pan: f32) -> DrumPad {
    DrumPad {
        note,
        name,
        group,
        sound,
        pan,
    }
}

/// The 16 pads, in note order. Matches General MIDI drum note numbers.
pub const DRUM_PADS: [DrumPad; 16] = [
    pad(36, "Kick", DrumGroup::Kick, Sound::Kick, 0.0),
    pad(37, "Rim", DrumGroup::Perc, Sound::Rim, 0.1),
    pad(38, "Snare", DrumGroup::Snare, Sound::Snare, 0.0),
    pad(39, "Clap", DrumGroup::Perc, Sound::Clap, -0.05),
    pad(40, "Snare 2", DrumGroup::Snare, Sound::Snare2, 0.05),
    pad(41, "Floor Tom", DrumGroup::Toms, Sound::Tom(82.0), -0.4),
    pad(42, "Closed Hat", DrumGroup::Hats, Sound::ClosedHat, 0.25),
    pad(43, "Floor Tom 2", DrumGroup::Toms, Sound::Tom(98.0), -0.3),
    pad(44, "Pedal Hat", DrumGroup::Hats, Sound::PedalHat, 0.25),
    pad(45, "Low Tom", DrumGroup::Toms, Sound::Tom(116.0), -0.15),
    pad(46, "Open Hat", DrumGroup::Hats, Sound::OpenHat, 0.25),
    pad(47, "Mid Tom", DrumGroup::Toms, Sound::Tom(138.0), 0.0),
    pad(48, "Hi-Mid Tom", DrumGroup::Toms, Sound::Tom(164.0), 0.15),
    pad(49, "Crash", DrumGroup::Cymbals, Sound::Crash, -0.25),
    pad(50, "High Tom", DrumGroup::Toms, Sound::Tom(196.0), 0.3),
    pad(51, "Ride", DrumGroup::Cymbals, Sound::Ride, 0.3),
];

const FIRST_NOTE: u8 = 36;
/// Classic analog cymbal oscillator frequencies (hertz).
const METAL_FREQS_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const SILENCE: f32 = 1e-4;
const CHOKE_TIME_S: f32 = 0.004;

#[derive(Debug, Clone)]
struct Params {
    group_levels: [f32; 6],
    kick_tune_semitones: f32,
    kick_decay: f32,
    snare_tone: f32,
    hats_decay: f32,
    kit_tune_semitones: f32,
    kit_decay: f32,
    gain_db: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            group_levels: [0.9, 0.75, 0.5, 0.7, 0.65, 0.45],
            kick_tune_semitones: 0.0,
            kick_decay: 1.0,
            snare_tone: 0.5,
            hats_decay: 1.0,
            kit_tune_semitones: 0.0,
            kit_decay: 1.0,
            gain_db: -6.0,
        }
    }
}

impl Params {
    fn level(&self, group: DrumGroup) -> f32 {
        self.group_levels[group as usize]
    }
}

/// Decay multiplier per sample that reaches -60 dB after `time_s`.
fn decay_coef(time_s: f32, sample_rate_hz: f32) -> f32 {
    (-(1000f32.ln()) / (time_s.max(0.001) * sample_rate_hz)).exp()
}

#[derive(Debug, Clone)]
struct PadVoice {
    active: bool,
    sound: Sound,
    gain_l: f32,
    gain_r: f32,
    /// Samples since the hit.
    t: u32,
    // Main ("body" or "metal") envelope.
    amp: f32,
    amp_coef: f32,
    // Secondary ("noise" or "click") envelope.
    noise_amp: f32,
    noise_coef: f32,
    // Pitch sweep for kick and toms, in hertz.
    pitch_env: f32,
    pitch_coef: f32,
    freq_start_hz: f32,
    freq_end_hz: f32,
    phase: f32,
    phase2: f32,
    freq2_hz: f32,
    tune: f32,
    metal: [Oscillator; 6],
    filter_a: Svf,
    filter_b: Svf,
    rng: Rng,
}

impl PadVoice {
    fn new(sound: Sound, seed: u32) -> Self {
        Self {
            active: false,
            sound,
            gain_l: 0.0,
            gain_r: 0.0,
            t: 0,
            amp: 0.0,
            amp_coef: 0.0,
            noise_amp: 0.0,
            noise_coef: 0.0,
            pitch_env: 0.0,
            pitch_coef: 0.0,
            freq_start_hz: 0.0,
            freq_end_hz: 0.0,
            phase: 0.0,
            phase2: 0.0,
            freq2_hz: 0.0,
            tune: 1.0,
            metal: Default::default(),
            filter_a: Svf::new(),
            filter_b: Svf::new(),
            rng: Rng::new(seed),
        }
    }

    // RT-SAFE
    fn trigger(&mut self, pad: &DrumPad, velocity: f32, p: &Params, sr: f32) {
        let level = p.level(pad.group) * (0.2 + 0.8 * velocity.clamp(0.0, 1.0));
        // Equal-power pan.
        let angle = (pad.pan + 1.0) * std::f32::consts::FRAC_PI_4;
        self.gain_l = level * angle.cos() * std::f32::consts::SQRT_2;
        self.gain_r = level * angle.sin() * std::f32::consts::SQRT_2;
        self.active = true;
        self.t = 0;
        self.phase = 0.0;
        self.phase2 = 0.0;
        self.filter_a.reset();
        self.filter_b.reset();
        self.tune = (p.kit_tune_semitones / 12.0).exp2();
        let kd = p.kit_decay;
        let tune = self.tune;
        match self.sound {
            Sound::Kick => {
                let kick_tune = tune * (p.kick_tune_semitones / 12.0).exp2();
                self.freq_start_hz = 160.0 * kick_tune;
                self.freq_end_hz = 48.0 * kick_tune;
                self.pitch_env = 1.0;
                self.pitch_coef = decay_coef(0.12, sr);
                self.amp = 1.0;
                self.amp_coef = decay_coef(0.5 * p.kick_decay * kd, sr);
                self.noise_amp = 0.5;
                self.noise_coef = decay_coef(0.006, sr);
            }
            Sound::Snare | Sound::Snare2 => {
                let (f1, f2, body_s, noise_s, center) = if self.sound == Sound::Snare {
                    (185.0, 330.0, 0.15, 0.25, 3_000.0)
                } else {
                    (235.0, 410.0, 0.09, 0.15, 4_500.0)
                };
                self.freq_start_hz = f1 * tune;
                self.freq2_hz = f2 * tune;
                self.amp = 1.0 - 0.6 * p.snare_tone;
                self.amp_coef = decay_coef(body_s * kd, sr);
                self.noise_amp = 0.4 + 0.6 * p.snare_tone;
                self.noise_coef = decay_coef(noise_s * kd, sr);
                self.filter_a.set(center * tune, 0.2, sr);
                self.filter_b.set(800.0, 0.0, sr);
            }
            Sound::Clap => {
                self.noise_amp = 1.0;
                self.noise_coef = decay_coef(0.25 * kd, sr);
                self.filter_a.set(1_100.0 * tune, 0.45, sr);
            }
            Sound::Rim => {
                self.freq_start_hz = 1_700.0 * tune;
                self.amp = 1.0;
                self.amp_coef = decay_coef(0.04 * kd, sr);
                self.noise_amp = 0.6;
                self.noise_coef = decay_coef(0.015, sr);
                self.filter_a.set(2_500.0 * tune, 0.5, sr);
            }
            Sound::Tom(base_hz) => {
                self.freq_start_hz = base_hz * 1.6 * tune;
                self.freq_end_hz = base_hz * tune;
                self.pitch_env = 1.0;
                self.pitch_coef = decay_coef(0.25, sr);
                self.amp = 1.0;
                self.amp_coef = decay_coef(0.6 * kd, sr);
                self.noise_amp = 0.15;
                self.noise_coef = decay_coef(0.03, sr);
            }
            Sound::ClosedHat | Sound::PedalHat | Sound::OpenHat => {
                let decay_s = match self.sound {
                    Sound::ClosedHat => 0.07,
                    Sound::PedalHat => 0.12,
                    _ => 0.6,
                };
                self.amp = 1.0;
                self.amp_coef = decay_coef(decay_s * p.hats_decay * kd, sr);
                self.filter_a.set(10_000.0 * tune, 0.3, sr);
                self.filter_b.set(7_000.0 * tune, 0.0, sr);
            }
            Sound::Crash => {
                self.amp = 1.0;
                self.amp_coef = decay_coef(1.8 * kd, sr);
                self.noise_amp = 0.6;
                self.noise_coef = decay_coef(1.2 * kd, sr);
                self.filter_b.set(4_000.0 * tune, 0.0, sr);
            }
            Sound::Ride => {
                self.amp = 1.0;
                self.amp_coef = decay_coef(1.5 * kd, sr);
                self.noise_amp = 0.15;
                self.noise_coef = decay_coef(0.4 * kd, sr);
                self.filter_a.set(5_500.0 * tune, 0.4, sr);
                self.filter_b.set(3_000.0 * tune, 0.0, sr);
            }
        }
    }

    // RT-SAFE
    fn choke(&mut self, sr: f32) {
        if self.active {
            let fast = decay_coef(CHOKE_TIME_S, sr);
            self.amp_coef = self.amp_coef.min(fast);
            self.noise_coef = self.noise_coef.min(fast);
        }
    }

    // RT-SAFE
    fn metal_sample(&mut self, sr: f32) -> f32 {
        let tune = self.tune;
        let mut sum = 0.0;
        for (osc, f) in self.metal.iter_mut().zip(METAL_FREQS_HZ) {
            sum += osc.next(Waveform::Square, f * tune / sr, 0.5);
        }
        sum / 6.0
    }

    // RT-SAFE
    fn next(&mut self, sr: f32) -> f32 {
        use std::f32::consts::TAU;
        let out = match self.sound {
            Sound::Kick | Sound::Tom(_) => {
                let f = self.freq_end_hz + (self.freq_start_hz - self.freq_end_hz) * self.pitch_env;
                self.pitch_env *= self.pitch_coef;
                self.phase = (self.phase + f / sr).fract();
                let body = (self.phase * TAU).sin() * self.amp;
                let click = self.rng.next_bipolar() * self.noise_amp;
                body + click
            }
            Sound::Snare | Sound::Snare2 => {
                self.phase = (self.phase + self.freq_start_hz / sr).fract();
                self.phase2 = (self.phase2 + self.freq2_hz / sr).fract();
                let body = ((self.phase * TAU).sin() + 0.6 * (self.phase2 * TAU).sin()) * 0.6;
                let n = self.rng.next_bipolar();
                let noise = self.filter_b.process(
                    self.filter_a.process(n, FilterMode::BandPass) * 2.0 + n * 0.3,
                    FilterMode::HighPass,
                );
                body * self.amp + noise * self.noise_amp
            }
            Sound::Clap => {
                // Three quick bursts, then a longer tail: a few hands at once.
                let burst_samples = (0.011 * sr) as u32;
                let env = if self.t < burst_samples * 3 {
                    let since = (self.t % burst_samples) as f32 / sr;
                    (-since / 0.003).exp()
                } else {
                    self.noise_amp
                };
                let n = self
                    .filter_a
                    .process(self.rng.next_bipolar(), FilterMode::BandPass);
                n * env * 2.5
            }
            Sound::Rim => {
                self.phase = (self.phase + self.freq_start_hz / sr).fract();
                let tone = (self.phase * TAU).sin() * self.amp * 0.7;
                let click = self
                    .filter_a
                    .process(self.rng.next_bipolar(), FilterMode::BandPass)
                    * self.noise_amp
                    * 2.0;
                tone + click
            }
            Sound::ClosedHat | Sound::PedalHat | Sound::OpenHat => {
                let metal = self.metal_sample(sr) + self.rng.next_bipolar() * 0.3;
                let band = self.filter_a.process(metal, FilterMode::BandPass);
                self.filter_b.process(band, FilterMode::HighPass) * self.amp * 4.0
            }
            Sound::Crash => {
                let metal = self.metal_sample(sr) * self.amp;
                let noise = self.rng.next_bipolar() * self.noise_amp;
                self.filter_b.process(metal + noise, FilterMode::HighPass) * 1.2
            }
            Sound::Ride => {
                let metal = self.metal_sample(sr);
                let ping = self.filter_a.process(metal, FilterMode::BandPass);
                let noise = self.rng.next_bipolar() * self.noise_amp;
                self.filter_b
                    .process(ping * 1.5 + metal * 0.3 + noise, FilterMode::HighPass)
                    * self.amp
                    * 2.5
            }
        };
        self.amp *= self.amp_coef;
        self.noise_amp *= self.noise_coef;
        self.t = self.t.saturating_add(1);
        let clap_bursting = self.sound == Sound::Clap && self.t < (0.04 * sr) as u32;
        if self.amp < SILENCE && self.noise_amp < SILENCE && !clap_bursting {
            self.active = false;
        }
        out
    }
}

/// The drum machine. Each pad has one voice; hitting a pad again restarts it.
pub struct DrumMachine {
    sample_rate_hz: f32,
    params: Params,
    voices: Vec<PadVoice>,
    gain: Smoother,
}

impl DrumMachine {
    pub fn new(sample_rate_hz: f32) -> Self {
        let params = Params::default();
        Self {
            sample_rate_hz,
            gain: Smoother::new(db_to_gain(params.gain_db), 0.02, sample_rate_hz),
            params,
            voices: DRUM_PADS
                .iter()
                .enumerate()
                .map(|(i, p)| PadVoice::new(p.sound, 0x1234_5678 ^ (i as u32 * 7919)))
                .collect(),
        }
    }

    /// Number of pads currently sounding. For tests and meters.
    pub fn active_pads(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

impl InstrumentProcessor for DrumMachine {
    fn note_on(&mut self, note: u8, velocity: f32) {
        if velocity <= 0.0 {
            return;
        }
        let Some(index) = note.checked_sub(FIRST_NOTE).map(usize::from) else {
            return;
        };
        let Some(pad) = DRUM_PADS.get(index) else {
            return;
        };
        // Closing the hi-hat cuts off a ringing open hat, like a real one.
        if matches!(pad.sound, Sound::ClosedHat | Sound::PedalHat) {
            let sr = self.sample_rate_hz;
            for v in self.voices.iter_mut().filter(|v| v.sound == Sound::OpenHat) {
                v.choke(sr);
            }
        }
        if let Some(v) = self.voices.get_mut(index) {
            v.trigger(pad, velocity, &self.params, self.sample_rate_hz);
        }
    }

    /// Drums play to the end of their decay; note-off is ignored.
    fn note_off(&mut self, _note: u8) {}

    fn all_notes_off(&mut self) {
        let sr = self.sample_rate_hz;
        for v in &mut self.voices {
            v.choke(sr);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = DRUM_PARAMS.get(index) else {
            return;
        };
        let p = &mut self.params;
        match spec.id {
            "kick.level" => p.group_levels[DrumGroup::Kick as usize] = value,
            "snare.level" => p.group_levels[DrumGroup::Snare as usize] = value,
            "hats.level" => p.group_levels[DrumGroup::Hats as usize] = value,
            "toms.level" => p.group_levels[DrumGroup::Toms as usize] = value,
            "perc.level" => p.group_levels[DrumGroup::Perc as usize] = value,
            "cymbals.level" => p.group_levels[DrumGroup::Cymbals as usize] = value,
            "kick.tune_semitones" => p.kick_tune_semitones = value,
            "kick.decay" => p.kick_decay = value,
            "snare.tone" => p.snare_tone = value,
            "hats.decay" => p.hats_decay = value,
            "kit.tune_semitones" => p.kit_tune_semitones = value,
            "kit.decay" => p.kit_decay = value,
            "master.gain_db" => {
                p.gain_db = value;
                self.gain.set_target(db_to_gain(value));
            }
            _ => {}
        }
    }

    fn control_change(&mut self, controller: u8, _value: u8) {
        if matches!(controller, 120 | 123) {
            self.all_notes_off();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate_hz;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut sum_l = 0.0;
            let mut sum_r = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let s = v.next(sr);
                sum_l += s * v.gain_l;
                sum_r += s * v.gain_r;
            }
            let g = self.gain.next_value();
            *l += sum_l * g;
            *r += sum_r * g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{assert_clean, peak, render};
    use daw_model::{Instrument, InstrumentKind, instrument::DRUM_PRESETS};

    const SR: f32 = 48_000.0;

    #[test]
    fn every_pad_sounds_and_decays_in_every_kit() {
        for preset in DRUM_PRESETS {
            let inst = Instrument::from_preset(InstrumentKind::Drums, preset.name).expect("kit");
            for pad in DRUM_PADS {
                let mut d = crate::create(&inst, SR);
                d.note_on(pad.note, 1.0);
                let (l, r) = render(d.as_mut(), SR, 4.0);
                assert_clean(&l, 1.0);
                assert_clean(&r, 1.0);
                assert!(
                    peak(&l[..4800]).max(peak(&r[..4800])) > 0.02,
                    "{} / {} is silent",
                    preset.name,
                    pad.name
                );
                let (tail, _) = render(d.as_mut(), SR, 0.1);
                assert!(
                    peak(&tail) < 1e-3,
                    "{} / {} rings on",
                    preset.name,
                    pad.name
                );
            }
        }
    }

    #[test]
    fn pads_cover_gm_notes_36_to_51() {
        for (i, pad) in DRUM_PADS.iter().enumerate() {
            assert_eq!(usize::from(pad.note), 36 + i);
        }
    }

    #[test]
    fn closed_hat_chokes_open_hat() {
        let mut d = DrumMachine::new(SR);
        d.note_on(46, 1.0);
        render(&mut d, SR, 0.05);
        d.note_on(42, 1.0);
        render(&mut d, SR, 0.15);
        assert!(!d.voices[10].active, "open hat still ringing");
    }

    #[test]
    fn notes_outside_the_kit_are_ignored() {
        let mut d = DrumMachine::new(SR);
        d.note_on(10, 1.0);
        d.note_on(100, 1.0);
        assert_eq!(d.active_pads(), 0);
    }

    #[test]
    fn typical_downbeat_does_not_clip() {
        // Kick, snare, closed hat, and crash on the same beat.
        let mut d = DrumMachine::new(SR);
        for note in [36, 38, 42, 49] {
            d.note_on(note, 1.0);
        }
        let (l, r) = render(&mut d, SR, 1.0);
        assert_clean(&l, 1.0);
        assert_clean(&r, 1.0);
    }
}
