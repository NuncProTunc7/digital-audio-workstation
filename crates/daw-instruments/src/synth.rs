//! Polyphonic subtractive synthesizer.
//!
//! Signal path per voice: two oscillators + sub + noise → state-variable
//! filter (with its own envelope, key tracking, and LFO) → amp envelope.
//! Mono mode keeps a stack of held notes for legato playing and glide.

use daw_dsp::{
    Adsr, AdsrParams, FilterMode, Lfo, Oscillator, Rng, Smoother, Svf, Waveform, db_to_gain,
    midi_to_hz,
};
use daw_model::instrument::SYNTH_PARAMS;

use crate::InstrumentProcessor;

const MAX_VOICES: usize = 16;
/// Filter cutoff and LFO update every this many samples (~0.7 ms at 48 kHz).
const CONTROL_BLOCK: usize = 32;
/// Per-voice level before the master gain, leaving headroom for chords.
const VOICE_GAIN: f32 = 0.25;
/// How far the filter envelope can move the cutoff, in octaves.
const FILTER_ENV_OCTAVES: f32 = 6.0;
/// How far the LFO can move the cutoff, in octaves.
const LFO_CUTOFF_OCTAVES: f32 = 3.0;
const MAX_HELD_NOTES: usize = 16;
const PITCH_BEND_RANGE_SEMITONES: f32 = 2.0;

#[derive(Debug, Clone)]
struct Params {
    osc1_wave: Waveform,
    osc1_level: f32,
    osc2_wave: Waveform,
    osc2_level: f32,
    osc2_semitones: f32,
    osc2_detune_cents: f32,
    pulse_width: f32,
    sub_level: f32,
    noise_level: f32,
    filter_mode: FilterMode,
    cutoff_hz: f32,
    resonance: f32,
    filter_env_amount: f32,
    key_track: f32,
    amp: AdsrParams,
    fenv: AdsrParams,
    lfo_rate_hz: f32,
    lfo_to_pitch_cents: f32,
    lfo_to_cutoff: f32,
    mono: bool,
    glide_s: f32,
    velocity_sensitivity: f32,
    gain_db: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            osc1_wave: Waveform::Saw,
            osc1_level: 0.8,
            osc2_wave: Waveform::Saw,
            osc2_level: 0.5,
            osc2_semitones: 0.0,
            osc2_detune_cents: 7.0,
            pulse_width: 0.5,
            sub_level: 0.0,
            noise_level: 0.0,
            filter_mode: FilterMode::LowPass,
            cutoff_hz: 2_000.0,
            resonance: 0.2,
            filter_env_amount: 0.3,
            key_track: 0.5,
            amp: AdsrParams::default(),
            fenv: AdsrParams::default(),
            lfo_rate_hz: 5.0,
            lfo_to_pitch_cents: 0.0,
            lfo_to_cutoff: 0.0,
            mono: false,
            glide_s: 0.0,
            velocity_sensitivity: 0.7,
            gain_db: -6.0,
        }
    }
}

#[derive(Debug, Clone)]
struct Voice {
    note: u8,
    velocity_gain: f32,
    /// Monotonic counter for "steal the oldest voice".
    started_at: u64,
    /// Note-off arrived while the sustain pedal was down.
    held_by_pedal: bool,
    /// Current and target pitch, as fractional MIDI notes (for glide).
    pitch: f32,
    target_pitch: f32,
    osc1: Oscillator,
    osc2: Oscillator,
    sub: Oscillator,
    filter: Svf,
    amp_env: Adsr,
    filter_env: Adsr,
}

impl Voice {
    fn new(sample_rate_hz: f32) -> Self {
        Self {
            note: 0,
            velocity_gain: 0.0,
            started_at: 0,
            held_by_pedal: false,
            pitch: 60.0,
            target_pitch: 60.0,
            osc1: Oscillator::new(),
            osc2: Oscillator::new(),
            sub: Oscillator::new(),
            filter: Svf::new(),
            amp_env: Adsr::new(AdsrParams::default(), sample_rate_hz),
            filter_env: Adsr::new(AdsrParams::default(), sample_rate_hz),
        }
    }

    fn is_active(&self) -> bool {
        self.amp_env.is_active()
    }
}

/// The synth. Create off the audio thread; everything after is real-time safe.
pub struct Synth {
    sample_rate_hz: f32,
    params: Params,
    voices: Vec<Voice>,
    lfo: Lfo,
    rng: Rng,
    gain: Smoother,
    note_counter: u64,
    sustain_pedal: bool,
    pitch_bend_semitones: f32,
    // Mono mode: held notes, most recent last. Fixed size; never reallocates.
    held: [u8; MAX_HELD_NOTES],
    held_len: usize,
    // Samples left until the next control-rate update.
    control_countdown: usize,
    lfo_value: f32,
}

impl Synth {
    pub fn new(sample_rate_hz: f32) -> Self {
        let params = Params::default();
        Self {
            sample_rate_hz,
            gain: Smoother::new(db_to_gain(params.gain_db), 0.02, sample_rate_hz),
            params,
            voices: (0..MAX_VOICES)
                .map(|_| Voice::new(sample_rate_hz))
                .collect(),
            lfo: Lfo::default(),
            rng: Rng::new(0x5EED),
            note_counter: 0,
            sustain_pedal: false,
            pitch_bend_semitones: 0.0,
            held: [0; MAX_HELD_NOTES],
            held_len: 0,
            control_countdown: 0,
            lfo_value: 0.0,
        }
    }

    /// Number of voices currently making sound. For tests and meters.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    // RT-SAFE
    fn apply_envelope_params(&mut self) {
        for v in &mut self.voices {
            v.amp_env.set_params(self.params.amp, self.sample_rate_hz);
            v.filter_env
                .set_params(self.params.fenv, self.sample_rate_hz);
        }
    }

    // RT-SAFE
    fn velocity_gain(&self, velocity: f32) -> f32 {
        let s = self.params.velocity_sensitivity;
        1.0 - s + s * velocity.clamp(0.0, 1.0)
    }

    // RT-SAFE
    fn start_voice(&mut self, index: usize, note: u8, velocity: f32, glide_from: Option<f32>) {
        self.note_counter += 1;
        let velocity_gain = self.velocity_gain(velocity);
        let Some(v) = self.voices.get_mut(index) else {
            return;
        };
        v.note = note;
        v.velocity_gain = velocity_gain;
        v.started_at = self.note_counter;
        v.held_by_pedal = false;
        v.target_pitch = f32::from(note);
        v.pitch = glide_from.unwrap_or(v.target_pitch);
        if !v.is_active() {
            // A silent voice can start fresh; a sounding one keeps its phase
            // and filter state so the steal doesn't click.
            v.filter.reset();
        }
        v.amp_env.note_on();
        v.filter_env.note_on();
    }

    // RT-SAFE
    fn pick_poly_voice(&self, note: u8) -> usize {
        // Same note already sounding: retrigger it rather than stack copies.
        if let Some(i) = self
            .voices
            .iter()
            .position(|v| v.is_active() && v.note == note)
        {
            return i;
        }
        if let Some(i) = self.voices.iter().position(|v| !v.is_active()) {
            return i;
        }
        // Steal the quietest released voice, else the oldest one.
        let released = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.amp_env.is_released())
            .min_by(|a, b| a.1.amp_env.level().total_cmp(&b.1.amp_env.level()));
        if let Some((i, _)) = released {
            return i;
        }
        self.voices
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.started_at)
            .map_or(0, |(i, _)| i)
    }

    // RT-SAFE
    fn mono_note_on(&mut self, note: u8, velocity: f32) {
        self.remove_held(note);
        if self.held_len == MAX_HELD_NOTES {
            self.held.copy_within(1.., 0);
            self.held_len -= 1;
        }
        if let Some(slot) = self.held.get_mut(self.held_len) {
            *slot = note;
            self.held_len += 1;
        }
        let voice_sounding = self
            .voices
            .first()
            .is_some_and(|v| v.is_active() && !v.amp_env.is_released());
        if voice_sounding && self.held_len > 1 {
            // Legato: glide to the new note without restarting envelopes.
            if let Some(v) = self.voices.first_mut() {
                v.note = note;
                v.target_pitch = f32::from(note);
            }
        } else {
            let from = self
                .voices
                .first()
                .filter(|v| v.is_active() && self.params.glide_s > 0.0)
                .map(|v| v.pitch);
            self.start_voice(0, note, velocity, from);
        }
    }

    // RT-SAFE
    fn mono_note_off(&mut self, note: u8) {
        self.remove_held(note);
        let Some(v) = self.voices.first_mut() else {
            return;
        };
        if v.note != note {
            return;
        }
        if self.held_len > 0 {
            // Fall back to the previous held note, legato.
            let previous = self.held[self.held_len - 1];
            v.note = previous;
            v.target_pitch = f32::from(previous);
        } else if self.sustain_pedal {
            v.held_by_pedal = true;
        } else {
            v.amp_env.note_off();
            v.filter_env.note_off();
        }
    }

    // RT-SAFE
    fn remove_held(&mut self, note: u8) {
        if let Some(pos) = self.held[..self.held_len].iter().position(|&n| n == note) {
            self.held.copy_within(pos + 1..self.held_len, pos);
            self.held_len -= 1;
        }
    }

    // RT-SAFE
    fn release_pedal_notes(&mut self) {
        for v in &mut self.voices {
            if v.held_by_pedal {
                v.held_by_pedal = false;
                v.amp_env.note_off();
                v.filter_env.note_off();
            }
        }
    }

    // RT-SAFE
    fn update_control_rate(&mut self) {
        self.lfo_value =
            self.lfo
                .advance(self.params.lfo_rate_hz, self.sample_rate_hz, CONTROL_BLOCK);
        let p = &self.params;
        for v in self.voices.iter_mut().filter(|v| v.is_active()) {
            let octaves = p.filter_env_amount * FILTER_ENV_OCTAVES * v.filter_env.level()
                + p.key_track * (v.pitch - 60.0) / 12.0
                + p.lfo_to_cutoff * LFO_CUTOFF_OCTAVES * self.lfo_value;
            let cutoff = p.cutoff_hz * octaves.exp2();
            v.filter.set(cutoff, p.resonance, self.sample_rate_hz);
        }
    }
}

impl InstrumentProcessor for Synth {
    fn note_on(&mut self, note: u8, velocity: f32) {
        if velocity <= 0.0 {
            self.note_off(note);
            return;
        }
        if self.params.mono {
            self.mono_note_on(note, velocity);
        } else {
            let index = self.pick_poly_voice(note);
            self.start_voice(index, note, velocity, None);
        }
    }

    fn note_off(&mut self, note: u8) {
        if self.params.mono {
            self.mono_note_off(note);
            return;
        }
        let pedal = self.sustain_pedal;
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.is_active() && v.note == note && !v.amp_env.is_released())
        {
            if pedal {
                v.held_by_pedal = true;
            } else {
                v.amp_env.note_off();
                v.filter_env.note_off();
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.held_len = 0;
        self.sustain_pedal = false;
        for v in &mut self.voices {
            v.held_by_pedal = false;
            v.amp_env.note_off();
            v.filter_env.note_off();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = SYNTH_PARAMS.get(index) else {
            return;
        };
        let p = &mut self.params;
        let mut envelopes_changed = false;
        let mut mode_changed = false;
        match spec.id {
            "osc1.wave" => p.osc1_wave = Waveform::from_index(value),
            "osc1.level" => p.osc1_level = value,
            "osc2.wave" => p.osc2_wave = Waveform::from_index(value),
            "osc2.level" => p.osc2_level = value,
            "osc2.semitones" => p.osc2_semitones = value,
            "osc2.detune_cents" => p.osc2_detune_cents = value,
            "osc.pulse_width" => p.pulse_width = value,
            "sub.level" => p.sub_level = value,
            "noise.level" => p.noise_level = value,
            "filter.mode" => p.filter_mode = FilterMode::from_index(value),
            "filter.cutoff_hz" => p.cutoff_hz = value,
            "filter.resonance" => p.resonance = value,
            "filter.env_amount" => p.filter_env_amount = value,
            "filter.key_track" => p.key_track = value,
            "amp.attack_s" => (p.amp.attack_s, envelopes_changed) = (value, true),
            "amp.decay_s" => (p.amp.decay_s, envelopes_changed) = (value, true),
            "amp.sustain" => (p.amp.sustain, envelopes_changed) = (value, true),
            "amp.release_s" => (p.amp.release_s, envelopes_changed) = (value, true),
            "fenv.attack_s" => (p.fenv.attack_s, envelopes_changed) = (value, true),
            "fenv.decay_s" => (p.fenv.decay_s, envelopes_changed) = (value, true),
            "fenv.sustain" => (p.fenv.sustain, envelopes_changed) = (value, true),
            "fenv.release_s" => (p.fenv.release_s, envelopes_changed) = (value, true),
            "lfo.rate_hz" => p.lfo_rate_hz = value,
            "lfo.to_pitch_cents" => p.lfo_to_pitch_cents = value,
            "lfo.to_cutoff" => p.lfo_to_cutoff = value,
            "voice.mode" => {
                let mono = value >= 0.5;
                mode_changed = mono != p.mono;
                p.mono = mono;
            }
            "voice.glide_s" => p.glide_s = value,
            "velocity.sensitivity" => p.velocity_sensitivity = value,
            "master.gain_db" => {
                p.gain_db = value;
                self.gain.set_target(db_to_gain(value));
            }
            _ => {}
        }
        if envelopes_changed {
            self.apply_envelope_params();
        }
        if mode_changed {
            self.all_notes_off();
        }
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            // Sustain pedal.
            64 => {
                let down = value >= 64;
                if self.sustain_pedal && !down {
                    self.sustain_pedal = false;
                    self.release_pedal_notes();
                } else {
                    self.sustain_pedal = down;
                }
            }
            // All notes off / all sound off.
            120 | 123 => self.all_notes_off(),
            _ => {}
        }
    }

    fn pitch_bend(&mut self, semitones: f32) {
        self.pitch_bend_semitones =
            semitones.clamp(-PITCH_BEND_RANGE_SEMITONES, PITCH_BEND_RANGE_SEMITONES);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate_hz;
        let glide_coef = if self.params.mono && self.params.glide_s > 0.0 {
            (-1.0 / (self.params.glide_s * sr)).exp()
        } else {
            0.0
        };
        let osc2_offset = self.params.osc2_semitones + self.params.osc2_detune_cents / 100.0;

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.control_countdown == 0 {
                self.update_control_rate();
                self.control_countdown = CONTROL_BLOCK;
            }
            self.control_countdown -= 1;

            let p = &self.params;
            let vibrato = p.lfo_to_pitch_cents / 100.0 * self.lfo_value;
            let bend = self.pitch_bend_semitones;
            let mut mix = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.is_active()) {
                v.pitch = v.target_pitch + (v.pitch - v.target_pitch) * glide_coef;
                let base = v.pitch + vibrato + bend;
                let inc1 = midi_to_hz(base) / sr;
                let inc2 = midi_to_hz(base + osc2_offset) / sr;

                let mut s = v.osc1.next(p.osc1_wave, inc1, p.pulse_width) * p.osc1_level;
                if p.osc2_level > 0.0 {
                    s += v.osc2.next(p.osc2_wave, inc2, p.pulse_width) * p.osc2_level;
                }
                if p.sub_level > 0.0 {
                    s += v.sub.next(Waveform::Sine, inc1 * 0.5, 0.5) * p.sub_level;
                }
                if p.noise_level > 0.0 {
                    s += self.rng.next_bipolar() * p.noise_level;
                }
                let filtered = v.filter.process(s, p.filter_mode);
                v.filter_env.next_sample();
                mix += filtered * v.amp_env.next_sample() * v.velocity_gain;
            }
            let out = mix * VOICE_GAIN * self.gain.next_value();
            *l += out;
            *r += out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{assert_clean, peak, render};
    use daw_model::{Instrument, InstrumentKind, instrument::SYNTH_PRESETS};

    const SR: f32 = 48_000.0;

    fn synth_with(preset: &str) -> Box<dyn InstrumentProcessor> {
        let inst = Instrument::from_preset(InstrumentKind::Synth, preset).expect("preset");
        crate::create(&inst, SR)
    }

    #[test]
    fn every_preset_plays_cleanly_and_falls_silent() {
        for preset in SYNTH_PRESETS {
            let mut s = synth_with(preset.name);
            for note in [48, 55, 60, 64, 67] {
                s.note_on(note, 1.0);
            }
            let (on, _) = render(s.as_mut(), SR, 1.0);
            assert_clean(&on, 1.0);
            assert!(peak(&on) > 0.01, "{} is silent", preset.name);
            for note in [48, 55, 60, 64, 67] {
                s.note_off(note);
            }
            // Longest release in the presets is under 2 s.
            render(s.as_mut(), SR, 4.0);
            let (tail, _) = render(s.as_mut(), SR, 0.1);
            assert!(peak(&tail) < 1e-4, "{} still sounding", preset.name);
        }
    }

    #[test]
    fn plays_the_right_pitch() {
        let mut s = synth_with("Init");
        // Pure sine, no filter movement, no second oscillator.
        for (id, v) in [
            ("osc1.wave", 0.0),
            ("osc2.level", 0.0),
            ("filter.cutoff_hz", 20_000.0),
            ("filter.env_amount", 0.0),
        ] {
            let i = SYNTH_PARAMS.iter().position(|p| p.id == id).expect("param");
            s.set_param(i, v);
        }
        s.note_on(69, 1.0);
        render(s.as_mut(), SR, 0.1);
        let (x, _) = render(s.as_mut(), SR, 1.0);
        let crossings = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((438..=442).contains(&crossings), "{crossings}");
    }

    #[test]
    fn more_notes_than_voices_steals_without_glitches() {
        let mut s = Synth::new(SR);
        for note in 30..80 {
            s.note_on(note, 0.8);
            let (x, _) = render(&mut s, SR, 0.01);
            assert_clean(&x, 1.0);
        }
        assert_eq!(s.active_voices(), MAX_VOICES);
    }

    #[test]
    fn mono_mode_uses_one_voice_and_returns_to_held_note() {
        let mut s = synth_with("Fat Bass");
        let mut synth = Synth::new(SR);
        // Reach the concrete type for introspection.
        let mono = SYNTH_PARAMS
            .iter()
            .position(|p| p.id == "voice.mode")
            .expect("param");
        synth.set_param(mono, 1.0);
        synth.note_on(40, 1.0);
        synth.note_on(43, 1.0);
        render(&mut synth, SR, 0.2);
        assert_eq!(synth.active_voices(), 1);
        synth.note_off(43);
        render(&mut synth, SR, 0.2);
        assert_eq!(synth.voices[0].note, 40);
        assert!(!synth.voices[0].amp_env.is_released());

        // And the boxed preset version renders cleanly.
        s.note_on(40, 1.0);
        s.note_on(47, 1.0);
        let (x, _) = render(s.as_mut(), SR, 0.5);
        assert_clean(&x, 1.0);
    }

    #[test]
    fn sustain_pedal_holds_notes_until_released() {
        let mut s = Synth::new(SR);
        s.control_change(64, 127);
        s.note_on(60, 1.0);
        render(&mut s, SR, 0.05);
        s.note_off(60);
        render(&mut s, SR, 1.0);
        assert_eq!(s.active_voices(), 1);
        s.control_change(64, 0);
        render(&mut s, SR, 2.0);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn output_is_deterministic() {
        let run = || {
            let mut s = synth_with("Soft Pad");
            s.note_on(60, 0.7);
            render(s.as_mut(), SR, 0.5).0
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn extreme_settings_stay_finite() {
        let mut s = Synth::new(SR);
        for (i, spec) in SYNTH_PARAMS.iter().enumerate() {
            s.set_param(i, spec.max as f32);
        }
        s.note_on(127, 1.0);
        s.note_on(0, 1.0);
        let (x, _) = render(&mut s, SR, 1.0);
        assert_clean(&x, 4.0);
    }
}
