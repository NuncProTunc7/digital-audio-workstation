//! Built-in instruments.
//!
//! Each instrument is created on a normal thread (it allocates its voices up
//! front) and then driven from the audio thread through [`InstrumentProcessor`],
//! whose methods never allocate, lock, or do I/O.

mod drums;
mod plugin;
mod sampler;
mod synth;

pub use drums::{DRUM_PADS, DrumGroup, DrumMachine, DrumPad};
pub use plugin::{PluginLoad, create_plugin, plugin_info};
pub use synth::Synth;

use daw_model::{Instrument, InstrumentKind, instrument::param_specs};

/// The real-time face of an instrument.
pub trait InstrumentProcessor: Send {
    /// `velocity` is 0.0–1.0.
    // RT-SAFE
    fn note_on(&mut self, note: u8, velocity: f32);
    // RT-SAFE
    fn note_off(&mut self, note: u8);
    /// Releases every note, including ones held by the sustain pedal.
    // RT-SAFE
    fn all_notes_off(&mut self);
    /// Sets a parameter by its index in `daw_model::instrument::param_specs`.
    // RT-SAFE
    fn set_param(&mut self, index: usize, value: f32);
    /// MIDI control change (0–127). Instruments ignore what they don't use.
    // RT-SAFE
    fn control_change(&mut self, _controller: u8, _value: u8) {}
    /// Pitch bend in semitones.
    // RT-SAFE
    fn pitch_bend(&mut self, _semitones: f32) {}
    /// Adds this instrument's output into the buffers (does not clear them).
    // RT-SAFE
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Sets a plugin instrument's parameter `id` (0–1). Built-in
    /// instruments ignore it.
    // RT-SAFE
    fn set_plugin_param(&mut self, _id: u32, _value: f64) {}
    /// Tempo, for plugins that follow it.
    // RT-SAFE
    fn set_tempo(&mut self, _bpm: f64) {}
}

/// Builds the processor for a track's instrument settings. Allocates; call
/// off the audio thread.
pub fn create(instrument: &Instrument, sample_rate_hz: f32) -> Box<dyn InstrumentProcessor> {
    let mut processor: Box<dyn InstrumentProcessor> = match instrument.kind {
        InstrumentKind::Synth => Box::new(Synth::new(sample_rate_hz)),
        InstrumentKind::Drums => Box::new(DrumMachine::new(sample_rate_hz)),
        // Audio tracks get their sound from audio clips, played by the engine.
        InstrumentKind::Audio => Box::new(Silent),
        InstrumentKind::Sampler => Box::new(sampler::Sampler::new(
            sample_rate_hz,
            instrument
                .sample_pack
                .as_deref()
                .map(|p| daw_sampler::load_cached(std::path::Path::new(p))),
        )),
        // Offline renders (export, freeze, A/B) get their own copy, from
        // the settings saved in the song.
        InstrumentKind::Plugin => return create_plugin(instrument, sample_rate_hz, None).0,
    };
    for (index, spec) in param_specs(instrument.kind).iter().enumerate() {
        let value = instrument.value(spec.id).unwrap_or(spec.default);
        processor.set_param(index, value as f32);
    }
    processor
}

/// The "instrument" of an audio track: ignores notes and adds nothing.
struct Silent;

impl InstrumentProcessor for Silent {
    // RT-SAFE
    fn note_on(&mut self, _note: u8, _velocity: f32) {}
    // RT-SAFE
    fn note_off(&mut self, _note: u8) {}
    // RT-SAFE
    fn all_notes_off(&mut self) {}
    // RT-SAFE
    fn set_param(&mut self, _index: usize, _value: f32) {}
    // RT-SAFE
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}

#[cfg(test)]
pub(crate) mod test_util {
    /// Renders `seconds` of audio in 128-sample blocks, like a sound card would.
    pub fn render(
        inst: &mut dyn super::InstrumentProcessor,
        sample_rate_hz: f32,
        seconds: f32,
    ) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * sample_rate_hz) as usize;
        let mut left = vec![0.0; n];
        let mut right = vec![0.0; n];
        for (l, r) in left.chunks_mut(128).zip(right.chunks_mut(128)) {
            inst.process(l, r);
        }
        (left, right)
    }

    pub fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    pub fn assert_clean(x: &[f32], max_peak: f32) {
        for (i, s) in x.iter().enumerate() {
            assert!(s.is_finite(), "sample {i} is {s}");
        }
        let p = peak(x);
        assert!(p <= max_peak, "peak {p} > {max_peak}");
    }
}

/// Everything the UI and Claude need to know about the built-in
/// instruments and effects.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Catalog {
    pub instruments: Vec<daw_model::instrument::InstrumentDescription>,
    pub effects: Vec<daw_model::effect::EffectDescription>,
    pub drum_pads: &'static [DrumPad],
}

pub fn catalog() -> Catalog {
    Catalog {
        instruments: daw_model::instrument::describe_instruments(),
        effects: daw_model::effect::describe_effects(),
        drum_pads: &DRUM_PADS,
    }
}
